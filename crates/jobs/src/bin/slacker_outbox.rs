//! Slack outbox drainer.
//!
//! Phase A: posts to Slack Workflow Builder webhooks. One workflow (and one
//! webhook URL) per outbox kind, because Workflow Builder webhooks bind 1:1
//! to a workflow with a fixed variable set. The payload column in each
//! `slack_outbox` row is already the flat JSON the workflow expects — we
//! POST it verbatim.
//!
//! Each loop iteration claims up to [`BATCH`] pending rows with
//! `FOR UPDATE SKIP LOCKED`, posts them one at a time, and marks them
//! delivered or failed inside the same transaction.

use std::time::Duration;

use clap::Parser;
use database::slack_outbox::{KIND_INCIDENT_OPEN, KIND_INCIDENT_RESOLVE, SlackOutbox};
use diesel_async::AsyncConnection;
use lloggs::{LoggingArgs, PreArgs};
use miette::IntoDiagnostic;
use tokio::{
	task::{self, JoinHandle},
	time::sleep,
};
use tracing::{debug, error, info, warn};

const BATCH: i64 = 10;
const TICK: Duration = Duration::from_secs(5);
const MAX_ATTEMPTS: i32 = 10;

/// Drainer configuration. One webhook URL per outbox kind (each Slack
/// workflow binds to a single variable set declared in its trigger, so
/// `incident_open` and `incident_resolve` need separate workflows), plus
/// the `PRIVATE_URL` base that the `link` variable points at.
///
/// `PRIVATE_URL` is required whenever any webhook URL is set. The link is
/// injected here at delivery time rather than at enqueue time so the
/// operator only has to configure one process. Localhost fallback would
/// produce a Slack message with a link that nobody can click — better to
/// refuse to start than to ship broken messages.
#[derive(Clone, Debug, Default)]
struct Config {
	open: Option<String>,
	resolve: Option<String>,
	private_url: Option<String>,
}

impl Config {
	fn from_env() -> miette::Result<Self> {
		let cfg = Self {
			open: std::env::var("SLACK_WEBHOOK_OPEN_URL").ok(),
			resolve: std::env::var("SLACK_WEBHOOK_RESOLVE_URL").ok(),
			private_url: std::env::var("PRIVATE_URL").ok(),
		};
		if cfg.any_hook() && cfg.private_url.is_none() {
			return Err(miette::miette!(
				"PRIVATE_URL must be set when any SLACK_WEBHOOK_*_URL is set \
				 — it's the base of the operator-facing admin UI that the \
				 `link` variable in each Slack message points at",
			));
		}
		Ok(cfg)
	}

	fn any_hook(&self) -> bool {
		self.open.is_some() || self.resolve.is_some()
	}

	fn url_for(&self, kind: &str) -> Option<&str> {
		match kind {
			KIND_INCIDENT_OPEN => self.open.as_deref(),
			KIND_INCIDENT_RESOLVE => self.resolve.as_deref(),
			_ => None,
		}
	}

	fn incident_link(&self, incident_id: uuid::Uuid) -> Option<String> {
		let base = self.private_url.as_deref()?.trim_end_matches('/');
		Some(format!("{base}/incidents/{incident_id}"))
	}
}

fn spawn(cfg: Config) -> JoinHandle<()> {
	let pool = database::init();
	if !cfg.any_hook() {
		info!("no SLACK_WEBHOOK_*_URL set; slack outbox drainer running in no-op mode");
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
						match deliver(&client, &cfg, &row).await {
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

/// Post the row's payload — augmented with a `link` derived from the row's
/// `incident_id` plus the configured `PRIVATE_URL` — to the workflow
/// webhook for this row's kind. Returns `Ok(())` for both real deliveries
/// and silent no-ops (no webhook configured for this kind, or unknown
/// kind logged as warn); the row gets marked delivered so the table
/// doesn't grow unbounded in non-Slack envs.
async fn deliver(
	client: &reqwest::Client,
	cfg: &Config,
	row: &SlackOutbox,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
	let Some(url) = cfg.url_for(&row.kind) else {
		if !is_known_kind(&row.kind) {
			warn!(id = %row.id, kind = %row.kind, "unknown slack outbox kind; dropping");
		} else {
			debug!(id = %row.id, kind = %row.kind, "no webhook url configured for kind; dropping");
		}
		return Ok(());
	};
	let mut payload = row.payload.clone();
	if let Some(obj) = payload.as_object_mut()
		&& let Some(link) = cfg.incident_link(row.incident_id)
	{
		obj.insert("link".to_string(), serde_json::Value::String(link));
	}
	let resp = client.post(url).json(&payload).send().await?;
	let status = resp.status();
	if status.is_success() {
		Ok(())
	} else {
		let body = resp.text().await.unwrap_or_default();
		Err(format!("slack returned {status}: {body}").into())
	}
}

fn is_known_kind(kind: &str) -> bool {
	matches!(kind, KIND_INCIDENT_OPEN | KIND_INCIDENT_RESOLVE)
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

	fn row(kind: &str, payload: Value) -> SlackOutbox {
		SlackOutbox {
			id: Uuid::nil(),
			created_at: Timestamp::now(),
			kind: kind.into(),
			incident_id: Uuid::nil(),
			issue_id: None,
			note_id: None,
			payload,
			delivered_at: None,
			attempts: 0,
			last_error: None,
		}
	}

	/// Spawns a tiny single-shot HTTP listener on a free port. The returned
	/// `(url, join, recorded)` lets a test point the drainer at it, await
	/// the request, and inspect the parsed JSON body. One std::net listener
	/// per test is enough — we don't need wiremock for this.
	fn one_shot_server() -> (
		String,
		std::thread::JoinHandle<()>,
		std::sync::Arc<std::sync::Mutex<Option<Value>>>,
	) {
		let recorded: std::sync::Arc<std::sync::Mutex<Option<Value>>> =
			std::sync::Arc::new(std::sync::Mutex::new(None));
		let recorded_clone = recorded.clone();
		let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		let url = format!("http://{}/hook", listener.local_addr().unwrap());
		let join = std::thread::spawn(move || {
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
				if let Some(he) = buf[..total].windows(4).position(|w| w == b"\r\n\r\n") {
					let headers = std::str::from_utf8(&buf[..he]).unwrap();
					let len = headers
						.lines()
						.find_map(|l| {
							l.strip_prefix("content-length: ")
								.or_else(|| l.strip_prefix("Content-Length: "))
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
		(url, join, recorded)
	}

	#[test]
	fn config_rejects_missing_private_url_when_any_hook_set() {
		// SAFETY: this test mutates global env, but cargo-test isolates
		// processes per binary so we won't collide with other tests in
		// this file (none of which read these vars).
		unsafe {
			std::env::set_var("SLACK_WEBHOOK_OPEN_URL", "http://example/");
			std::env::remove_var("SLACK_WEBHOOK_RESOLVE_URL");
			std::env::remove_var("PRIVATE_URL");
		}
		let err = Config::from_env().expect_err("must require PRIVATE_URL");
		assert!(err.to_string().contains("PRIVATE_URL"));
		unsafe {
			std::env::remove_var("SLACK_WEBHOOK_OPEN_URL");
		}
	}

	#[test]
	fn config_ok_when_no_hooks_set_even_without_private_url() {
		unsafe {
			std::env::remove_var("SLACK_WEBHOOK_OPEN_URL");
			std::env::remove_var("SLACK_WEBHOOK_RESOLVE_URL");
			std::env::remove_var("PRIVATE_URL");
		}
		let cfg = Config::from_env().expect("no-op mode is fine");
		assert!(!cfg.any_hook());
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_with_no_hooks_is_a_noop() {
		let r = row(KIND_INCIDENT_OPEN, serde_json::json!({}));
		deliver(&reqwest::Client::new(), &Config::default(), &r)
			.await
			.expect("noop ok");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_injects_link_from_private_url() {
		let (url, server, recorded) = one_shot_server();
		let incident_id = Uuid::new_v4();
		let cfg = Config {
			open: Some(url),
			resolve: None,
			private_url: Some("https://canopy.example.ts.net".into()),
		};
		let mut r = row(
			KIND_INCIDENT_OPEN,
			serde_json::json!({
				"server": "Prod",
				"severity": "Error",
				"source_ref": "canopy/reachability",
				"message": "boom",
			}),
		);
		r.incident_id = incident_id;
		deliver(&reqwest::Client::new(), &cfg, &r)
			.await
			.expect("deliver ok");
		server.join().unwrap();

		let got = recorded.lock().unwrap().clone().expect("got a request");
		assert_eq!(got["server"], "Prod");
		assert_eq!(got["severity"], "Error");
		assert_eq!(got["source_ref"], "canopy/reachability");
		assert_eq!(got["message"], "boom");
		assert_eq!(
			got["link"],
			format!("https://canopy.example.ts.net/incidents/{incident_id}"),
			"link injected at delivery time from PRIVATE_URL and row.incident_id",
		);
		assert!(got.get("blocks").is_none(), "no blocks wrapper");
		assert!(got.get("text").is_none(), "no text wrapper");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_overrides_stale_link_in_payload() {
		// A row with a `link` already in its payload (e.g. from an older
		// drainer version) gets that link overwritten by the freshly
		// computed one.
		let (url, server, recorded) = one_shot_server();
		let incident_id = Uuid::new_v4();
		let cfg = Config {
			open: Some(url),
			resolve: None,
			private_url: Some("https://new.example/".into()),
		};
		let mut r = row(
			KIND_INCIDENT_OPEN,
			serde_json::json!({ "link": "https://stale.example/old" }),
		);
		r.incident_id = incident_id;
		deliver(&reqwest::Client::new(), &cfg, &r)
			.await
			.expect("deliver ok");
		server.join().unwrap();

		let got = recorded.lock().unwrap().clone().unwrap();
		assert_eq!(
			got["link"],
			format!("https://new.example/incidents/{incident_id}")
		);
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_routes_resolve_kind_to_resolve_url() {
		// Open URL points at a port nothing's listening on; if the drainer
		// accidentally routed by anything other than `kind`, the test would
		// fail by hanging or by hitting connection-refused.
		let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		let resolve_url = format!("http://{}/hook", listener.local_addr().unwrap());
		let server = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			use std::io::{Read, Write};
			let mut buf = [0u8; 4096];
			let _ = stream.read(&mut buf);
			stream
				.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
				.unwrap();
		});

		let cfg = Config {
			open: Some("http://127.0.0.1:1/open-should-not-be-hit".into()),
			resolve: Some(resolve_url),
			private_url: Some("https://canopy.test".into()),
		};
		let r = row(KIND_INCIDENT_RESOLVE, serde_json::json!({"server": "x", "by": "me"}));
		deliver(&reqwest::Client::new(), &cfg, &r)
			.await
			.expect("deliver ok");
		server.join().unwrap();
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_returns_error_on_non_2xx() {
		let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
		let url = format!("http://{}/hook", listener.local_addr().unwrap());
		let server = std::thread::spawn(move || {
			let (mut stream, _) = listener.accept().unwrap();
			use std::io::{Read, Write};
			let mut buf = [0u8; 4096];
			let _ = stream.read(&mut buf);
			stream
				.write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 5\r\n\r\nnope!")
				.unwrap();
		});

		let cfg = Config {
			open: Some(url),
			resolve: None,
			private_url: Some("https://canopy.test".into()),
		};
		let r = row(KIND_INCIDENT_OPEN, serde_json::json!({}));
		let err = deliver(&reqwest::Client::new(), &cfg, &r)
			.await
			.expect_err("should error");
		server.join().unwrap();
		assert!(err.to_string().contains("500"), "error mentions status");
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn deliver_drops_unknown_kind_as_noop() {
		let cfg = Config {
			open: Some("http://127.0.0.1:1/should-not-be-hit".into()),
			resolve: None,
			private_url: Some("https://canopy.test".into()),
		};
		let r = row("bogus_kind", serde_json::json!({}));
		deliver(&reqwest::Client::new(), &cfg, &r)
			.await
			.expect("noop ok");
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

	let cfg = Config::from_env()?;
	spawn(cfg).await.into_diagnostic()?;
	Ok(())
}
