//! Slack outbox drainer.
//!
//! Phase A: posts to `SLACK_WEBHOOK_URL` (a Slack Workflow Builder incoming
//! webhook). The webhook is single-channel, single-direction, single-shape
//! — it accepts a JSON body and returns `200 ok` with no message metadata.
//! No threading, no inbound. Phase B will swap delivery for `chat.postMessage`
//! if a bot token is configured.
//!
//! Each loop iteration claims up to `BATCH` pending rows with
//! `FOR UPDATE SKIP LOCKED`, posts them one at a time, and marks them
//! delivered or failed inside the same transaction.

use std::time::Duration;

use clap::Parser;
use database::slack_outbox::SlackOutbox;
use diesel_async::AsyncConnection;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use serde_json::json;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

const BATCH: i64 = 10;
const TICK: Duration = Duration::from_secs(5);
const MAX_ATTEMPTS: i32 = 10;

pub fn spawn() -> JoinHandle<()> {
	let pool = database::init();
	let webhook = std::env::var("SLACK_WEBHOOK_URL").ok();
	if webhook.is_none() {
		info!("SLACK_WEBHOOK_URL not set; slack outbox drainer running in no-op mode");
	}
	let client = reqwest::Client::new();
	task::spawn(async move {
		loop {
			sleep(TICK).await;
			let Ok(mut db) = pool.get().await else {
				error!("Failed to get database connection");
				continue;
			};

			let result = db
				.transaction::<_, commons_errors::AppError, _>(async |conn| {
					let claimed = SlackOutbox::claim_pending(conn, BATCH).await?;
					for row in claimed {
						if row.attempts >= MAX_ATTEMPTS {
							warn!(
								id = %row.id,
								attempts = row.attempts,
								"giving up on slack outbox row"
							);
							SlackOutbox::mark_failed(conn, row.id, "max attempts exceeded").await?;
							continue;
						}
						match deliver(&client, webhook.as_deref(), &row).await {
							Ok(()) => SlackOutbox::mark_delivered(conn, row.id).await?,
							Err(err) => {
								warn!(id = %row.id, %err, "slack delivery failed");
								SlackOutbox::mark_failed(conn, row.id, &err.to_string()).await?;
							}
						}
					}
					Ok(())
				})
				.await;
			if let Err(err) = result {
				error!("slack outbox tx failed: {err}");
			}
		}
	})
}

/// Post the row's blocks payload to the webhook. If `webhook` is `None` we
/// pretend it succeeded — the row gets marked delivered and we move on.
/// That keeps non-Slack-configured environments from accumulating an
/// unbounded backlog.
async fn deliver(
	client: &reqwest::Client,
	webhook: Option<&str>,
	row: &SlackOutbox,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let Some(url) = webhook else {
		debug!(id = %row.id, "slack webhook not configured; dropping row");
		return Ok(());
	};
	// Workflow Builder webhooks accept the same shape as legacy
	// incoming-webhooks: a top-level object with a `blocks` array, plus
	// an optional `text` fallback used by notifications.
	let body = json!({
		"text": fallback_text(row),
		"blocks": row.payload,
	});
	let resp = client.post(url).json(&body).send().await?;
	let status = resp.status();
	if status.is_success() {
		Ok(())
	} else {
		let body = resp.text().await.unwrap_or_default();
		Err(format!("slack returned {status}: {body}").into())
	}
}

/// Best-effort plain-text fallback for screen readers / push notifications,
/// pulled from the first header block. Slack falls back to this when it
/// can't render blocks (e.g. in the mobile lock-screen notification).
fn fallback_text(row: &SlackOutbox) -> String {
	let blocks = row.payload.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
	for b in blocks {
		if b.get("type").and_then(|v| v.as_str()) == Some("header")
			&& let Some(text) = b.pointer("/text/text").and_then(|v| v.as_str())
		{
			return text.to_string();
		}
	}
	format!("canopy: {}", row.kind)
}

#[derive(Debug, Parser)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,
}

#[cfg(test)]
mod tests {
	use super::*;
	use jiff::Timestamp;
	use serde_json::Value;
	use uuid::Uuid;

	fn row(payload: Value) -> SlackOutbox {
		SlackOutbox {
			id: Uuid::nil(),
			created_at: Timestamp::now(),
			kind: "incident_open".into(),
			incident_id: Uuid::nil(),
			issue_id: None,
			note_id: None,
			payload,
			delivered_at: None,
			attempts: 0,
			last_error: None,
		}
	}

	#[test]
	fn fallback_text_pulls_first_header_block() {
		let r = row(serde_json::json!([
			{ "type": "header", "text": { "type": "plain_text", "text": "🚨 hi" } },
			{ "type": "section", "text": { "type": "mrkdwn", "text": "body" } },
		]));
		assert_eq!(fallback_text(&r), "🚨 hi");
	}

	#[test]
	fn fallback_text_defaults_to_kind_when_no_header() {
		let r = row(serde_json::json!([
			{ "type": "section", "text": { "type": "mrkdwn", "text": "body" } },
		]));
		assert_eq!(fallback_text(&r), "canopy: incident_open");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_no_webhook_succeeds_as_noop() {
		let r = row(serde_json::json!([]));
		deliver(&reqwest::Client::new(), None, &r)
			.await
			.expect("noop ok");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_posts_to_webhook_and_includes_blocks() {
		use std::sync::{Arc, Mutex};

		let recorded: Arc<Mutex<Option<Value>>> = Arc::new(Mutex::new(None));
		let recorded_clone = recorded.clone();

		// Tiny single-shot HTTP server. We don't pull in axum/wiremock for
		// one test — std::net + a hand-parsed POST is enough.
		let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		let addr = listener.local_addr().unwrap();
		let url = format!("http://{addr}/hook");
		let server = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			use std::io::{Read, Write};
			let mut buf = vec![0u8; 8192];
			let mut total = 0;
			loop {
				let n = stream.read(&mut buf[total..]).unwrap();
				if n == 0 {
					break;
				}
				total += n;
				let header_end = buf[..total].windows(4).position(|w| w == b"\r\n\r\n");
				if let Some(he) = header_end {
					let headers = std::str::from_utf8(&buf[..he]).unwrap();
					let len = headers
						.lines()
						.find_map(|l| l.strip_prefix("content-length: "))
						.or_else(|| {
							headers
								.lines()
								.find_map(|l| l.strip_prefix("Content-Length: "))
						})
						.unwrap_or("0")
						.parse::<usize>()
						.unwrap();
					if total - he - 4 >= len {
						let body = &buf[he + 4..he + 4 + len];
						let v: Value = serde_json::from_slice(body).unwrap();
						*recorded_clone.lock().unwrap() = Some(v);
						stream
							.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
							.unwrap();
						break;
					}
				}
			}
		});

		let r = row(serde_json::json!([
			{ "type": "header", "text": { "type": "plain_text", "text": "🚨 hi" } },
		]));
		deliver(&reqwest::Client::new(), Some(&url), &r)
			.await
			.expect("deliver ok");
		server.join().unwrap();

		let got = recorded.lock().unwrap().clone().expect("got a request");
		assert_eq!(got["text"], "🚨 hi");
		assert!(got["blocks"].is_array());
		assert_eq!(got["blocks"][0]["type"], "header");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_returns_error_on_non_2xx() {
		let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		let addr = listener.local_addr().unwrap();
		let url = format!("http://{addr}/hook");
		let server = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			use std::io::{Read, Write};
			let mut buf = [0u8; 4096];
			let _ = stream.read(&mut buf);
			stream
				.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\n\r\nnope!")
				.unwrap();
		});

		let r = row(serde_json::json!([]));
		let err = deliver(&reqwest::Client::new(), Some(&url), &r)
			.await
			.expect_err("should error");
		server.join().unwrap();
		let msg = err.to_string();
		assert!(msg.contains("500"), "error mentions status: {msg}");
	}
}

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}

	spawn().await.into_diagnostic()?;
	Ok(())
}
