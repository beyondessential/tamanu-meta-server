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
