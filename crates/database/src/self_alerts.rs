//! Self-alerts: canopy reporting problems with its own operation.
//!
//! Spec: `.workhorse/specs/private-server/self-alerts.md` (id `SELF`).
//!
//! Each condition is one coalescing issue on the nil "Canopy" server —
//! which is never grouped, so these never touch the per-group incident
//! flow. Notification goes straight to the Slack outbox instead
//! ([`crate::slack_outbox::KIND_SELF_ALERT_OPEN`] /
//! [`KIND_SELF_ALERT_RESOLVE`](crate::slack_outbox::KIND_SELF_ALERT_RESOLVE)),
//! with the same flap grace incidents get: sub-Critical raises wait out
//! [`GRACE`] before shipping, and a recovery inside that window cancels the
//! pending open so Slack hears nothing at all.

use commons_errors::Result;
use commons_types::issue::Severity;
use diesel_async::AsyncPgConnection;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use crate::issues::{Issue, NewEvent};
use crate::slack_outbox::{KIND_SELF_ALERT_OPEN, KIND_SELF_ALERT_RESOLVE, SlackOutbox, vars};
use crate::statuses::CANOPY_SOURCE;

/// An operator-notification delivery permanently failed (the drainer gave up
/// on an outbox row). No automatic recovery: stays until operator-resolved.
pub const SLACK_DELIVERY_FAILURE_REF: &str = "slack-delivery-failure";

/// Flap grace for sub-Critical self-alerts, mirroring the per-group
/// `slack_open_delay` default.
pub const GRACE: SignedDuration = SignedDuration::from_secs(3 * 60);

/// Raise (or re-affirm) a self-alert. Files the coalescing event against the
/// nil server; on the not-alerting → alerting transition — including a
/// re-raise of an issue an operator resolved while the condition still held —
/// enqueues the Slack notification. Repeated raises while alerting change
/// nothing Slack-side.
pub async fn raise(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	severity: Severity,
	title: &str,
	message: &str,
) -> Result<Issue> {
	let was_alerting = current(conn, r#ref)
		.await?
		.map(|i| i.active && i.resolved_at.is_none())
		.unwrap_or(false);

	let issue = NewEvent {
		source: CANOPY_SOURCE.into(),
		r#ref: r#ref.into(),
		severity: Some(severity),
		description: Some(title.into()),
		message: message.into(),
		active: Some(true),
		occurred_at: None,
	}
	.save(conn, Uuid::nil(), None)
	.await?;

	if !was_alerting {
		let deliver_after = if severity == Severity::Critical {
			Timestamp::now()
		} else {
			Timestamp::now() + GRACE
		};
		SlackOutbox::enqueue(
			conn,
			KIND_SELF_ALERT_OPEN,
			None,
			Some(issue.id),
			None,
			vars::self_alert_open(severity, r#ref, title, message),
			deliver_after,
		)
		.await?;
	}
	Ok(issue)
}

/// Recover a self-alert. Writes nothing when the alert isn't active. On the
/// active → inactive transition, cancels a still-pending open (a flap inside
/// [`GRACE`] makes no Slack noise) or enqueues the recovery notification —
/// unless an operator had already resolved the issue, in which case Slack
/// stays quiet too.
pub async fn recover(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	message: &str,
) -> Result<Option<Issue>> {
	let Some(existing) = current(conn, r#ref).await? else {
		return Ok(None);
	};
	if !existing.active {
		return Ok(None);
	}
	let was_alerting = existing.resolved_at.is_none();

	let issue = NewEvent {
		source: CANOPY_SOURCE.into(),
		r#ref: r#ref.into(),
		severity: Some(Severity::Info),
		description: None,
		message: message.into(),
		active: Some(false),
		occurred_at: None,
	}
	.save(conn, Uuid::nil(), None)
	.await?;

	if was_alerting {
		let cancelled = SlackOutbox::cancel_pending_self_alert_open(
			conn,
			issue.id,
			"cancelled: self-alert recovered before the open had been delivered to Slack",
		)
		.await?;
		if cancelled == 0 {
			SlackOutbox::enqueue(
				conn,
				KIND_SELF_ALERT_RESOLVE,
				None,
				Some(issue.id),
				None,
				vars::self_alert_resolve(r#ref, message),
				Timestamp::now(),
			)
			.await?;
		}
	}
	Ok(Some(issue))
}

/// The one issue for this condition, if it has ever been raised.
pub async fn current(conn: &mut AsyncPgConnection, r#ref: &str) -> Result<Option<Issue>> {
	Ok(
		Issue::list_by_source_ref(conn, CANOPY_SOURCE, r#ref, &[Uuid::nil()])
			.await?
			.into_iter()
			.next(),
	)
}

/// All self-alert issues (the nil server's), newest activity first. The
/// operator UI's alerts view; the fleet issue listings exclude these.
pub async fn list(conn: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Issue>> {
	use crate::schema::issues::dsl;
	use diesel::prelude::*;
	use diesel_async::RunQueryDsl;

	dsl::issues
		.select(Issue::as_select())
		.filter(dsl::server_id.eq(Some(Uuid::nil())))
		.order(dsl::last_seen.desc())
		.limit(limit)
		.load(conn)
		.await
		.map_err(commons_errors::AppError::from)
}
