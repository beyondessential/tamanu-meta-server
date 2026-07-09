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
use commons_types::status::CheckResult;
use diesel_async::AsyncPgConnection;

use crate::issues::{CheckFiling, FilingScope, Issue, file_check, get_global_issue};

/// An operator-notification delivery permanently failed (the drainer gave up
/// on an outbox row). No automatic recovery: stays until operator-resolved.
pub const SLACK_DELIVERY_FAILURE_REF: &str = "slack-delivery-failure";

pub const SLACK_DELIVERY_FAILURE_DOC: &str = "## Description

The Slack outbox drainer gave up delivering a notification after exhausting its retries — operators may be missing incident notices.

## Results

- **fail** — at least one outbox row was abandoned; recovers when a later delivery succeeds.

## Solve

Check the abandoned row's last error and response in the slack_outbox table, and the webhook URLs in the drainer's configuration. Slack workflow-trigger changes are the usual cause.";

/// Raise (or re-affirm) a self-alert: file the coalescing canopy-wide
/// check with the given observation, registering its catalog entry with
/// the policy the condition warrants (first sight only — operator edits
/// stick). The incident machinery handles notification — a fresh
/// effective failure opens (or joins) the canopy-wide incident, and
/// repeated raises while alerting change nothing Slack-side.
#[allow(clippy::too_many_arguments)]
pub async fn raise(
	conn: &mut AsyncPgConnection,
	r#ref: &str,
	observed: CheckResult,
	default_ceiling: CheckResult,
	default_escalates: bool,
	documentation: Option<&str>,
	title: &str,
	message: &str,
) -> Result<Issue> {
	file_check(
		conn,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: FilingScope::Global,
			check: r#ref,
			observed,
			title: Some(title),
			message,
			detail: None,
			default_ceiling,
			default_escalates,
			documentation,
		},
	)
	.await
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

	// The catalog entry exists (the active issue implies a prior raise
	// registered it), so the defaults here are inert.
	let issue = file_check(
		conn,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: FilingScope::Global,
			check: r#ref,
			observed: CheckResult::Passed,
			title: None,
			message,
			detail: None,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: None,
		},
	)
	.await?;
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
