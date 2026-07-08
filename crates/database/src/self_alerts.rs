//! Self-alerts: canopy reporting problems with its own operation.
//!
//! Spec: `.workhorse/specs/private-server/self-alerts.md` (id `SELF`).
//!
//! Each condition is one coalescing canopy-wide issue — scoped to
//! neither a server nor a group. Notification rides the incident
//! machinery: an error-or-worse condition opens an incident on the
//! canopy-wide target, with the same grace, escalation, and recovery
//! rules as any group incident (see
//! [`crate::issues::raise_global_event`]).

use commons_errors::Result;
use commons_types::issue::Severity;
use diesel_async::AsyncPgConnection;

use crate::issues::{Issue, get_global_issue, raise_global_event};

/// An operator-notification delivery permanently failed (the drainer gave up
/// on an outbox row). No automatic recovery: stays until operator-resolved.
pub const SLACK_DELIVERY_FAILURE_REF: &str = "slack-delivery-failure";

/// Raise (or re-affirm) a self-alert. Files the coalescing canopy-wide
/// issue; the incident machinery handles notification — a fresh
/// error-or-worse condition opens (or joins) the canopy-wide incident,
/// and repeated raises while alerting change nothing Slack-side.
pub async fn raise(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	severity: Severity,
	title: &str,
	message: &str,
) -> Result<Issue> {
	raise_global_event(conn, r#ref, severity, Some(title), message, true).await
}

/// Recover a self-alert. Writes nothing when the alert isn't active. On
/// the active → inactive transition the issue leaves the canopy-wide
/// incident, which closes (and notifies, or cancels a still-pending open)
/// once its last contributor is gone.
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

	let issue = raise_global_event(conn, r#ref, Severity::Info, None, message, false).await?;
	Ok(Some(issue))
}

/// The one issue for this condition, if it has ever been raised.
pub async fn current(conn: &mut AsyncPgConnection, r#ref: &str) -> Result<Option<Issue>> {
	get_global_issue(conn, r#ref).await
}

/// All self-alert issues (the canopy-wide ones), newest activity first.
/// The operator UI's alerts view; the fleet issue listings exclude these.
pub async fn list(conn: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Issue>> {
	use crate::schema::issues::dsl;
	use diesel::prelude::*;
	use diesel_async::RunQueryDsl;

	dsl::issues
		.select(Issue::as_select())
		.filter(dsl::server_id.is_null().and(dsl::server_group_id.is_null()))
		.order(dsl::last_seen.desc())
		.limit(limit)
		.load(conn)
		.await
		.map_err(commons_errors::AppError::from)
}
