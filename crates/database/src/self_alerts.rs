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
use commons_types::dns::ManagedZone;
use commons_types::status::CheckResult;
use diesel_async::AsyncPgConnection;

use crate::issues::{CheckFiling, Issue, Scope, file_check, get_global_issue};

/// An operator-notification delivery permanently failed (the drainer gave up
/// on an outbox row). No automatic recovery: stays until operator-resolved.
pub const SLACK_DELIVERY_FAILURE_REF: &str = "slack-delivery-failure";

pub const SLACK_DELIVERY_FAILURE_DOC: &str = "## Description

The Slack outbox drainer gave up delivering a notification after exhausting its retries — operators may be missing incident notices.

## Results

- **fail** — at least one outbox row was abandoned; recovers when a later delivery succeeds.

## Solve

Check the abandoned row's last error and response in the slack_outbox table, and the webhook URLs in the drainer's configuration. Slack workflow-trigger changes are the usual cause.";

/// One or more catalogued checks have gone unreported across the whole
/// fleet for the stale-alert window. Coalescing: one alert lists them all.
/// Recovers when none remain — each having been reported again or
/// decommissioned.
pub const STALE_CHECKS_REF: &str = "stale-healthchecks";

pub const STALE_CHECKS_DOC: &str = "## Description

One or more catalogued healthchecks have not been reported by any server in the fleet for 30 days. A check that has gone away everywhere is usually a reporter that was retired or renamed; its stale state lingers until an operator decommissions it.

## Results

- **warn** — at least one check is unreported fleet-wide for 30 days; recovers when none remain (each reported again or decommissioned).

## Solve

Review the checks listed in the operator UI's healthcheck settings and decommission the ones that are gone for good; a decommissioned check's stale issues are cleared fleet-wide.";

/// Canopy's configured DNS zones don't cover the domains groups have been
/// given — either the configuration is unreadable, or a zone has left it while
/// claims inside it stand. Coalescing: one alert lists every affected group.
/// Recovers when every live group's claims sit in a configured zone again.
// spec: DOM#when-the-zone-configuration-changes
pub const DNS_ZONE_COVERAGE_REF: &str = "dns-zone-coverage";

pub const DNS_ZONE_COVERAGE_DOC: &str = "## Description

Canopy holds the DNS zones it may write records in as deployment configuration, and a group domain is only actionable while a configured zone covers it. This alert means at least one group is now depending on a domain Canopy cannot reach — usually a zone removed from the configuration while its claims stood, or a configuration that no longer parses.

Claims are never dropped for this: the group keeps the domain, and it keeps excluding other groups from overlapping it. What stops is Canopy acting on any name beneath it.

## Results

- **warn** — some claims fall outside the configured zones while others are covered, which is what removing one zone of several looks like. Either restore the zone to the configuration or release the claims that no longer belong.
- **fail** — the configuration is unreadable, or there are no zones at all while domains stand claimed. Canopy can serve no DNS or TLS for any group until it is fixed.

## Solve

Compare the zone list in Canopy's deployment configuration against the domains reported in the alert message. Restore the missing zone if its removal was accidental; if it was deliberate, release the claims on each group's page. A configuration that does not parse is reported with the parse error — fix the entry and restart.";

/// Evaluate the fleet-wide check-liveness condition and raise or recover
/// the coalescing [`STALE_CHECKS_REF`] self-alert. Runs after liveness is
/// reconciled so it reads fresh `last_seen`. Returns the affected issue,
/// if any.
pub async fn sweep_stale_healthchecks(conn: &mut AsyncPgConnection) -> Result<Option<Issue>> {
	use crate::check_policies::{CheckPolicy, STALE_ALERT_HOURS};

	let cutoff = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(STALE_ALERT_HOURS);
	let quiet = CheckPolicy::gone_quiet(conn, cutoff).await?;
	if quiet.is_empty() {
		return recover(
			conn,
			STALE_CHECKS_REF,
			"all catalogued checks are reporting again or decommissioned",
		)
		.await;
	}

	let names: Vec<String> = quiet
		.iter()
		.map(|p| format!("{}/{}", p.source, p.check_name))
		.collect();
	let message = format!(
		"{} healthcheck(s) unreported fleet-wide for 30 days: {}",
		quiet.len(),
		names.join(", "),
	);
	raise(
		conn,
		STALE_CHECKS_REF,
		CheckResult::Warning,
		CheckResult::Warning,
		false,
		Some(STALE_CHECKS_DOC),
		"Healthchecks gone quiet",
		&message,
	)
	.await
	.map(Some)
}

/// Evaluate the DNS zone coverage condition and raise or recover the coalescing
/// [`DNS_ZONE_COVERAGE_REF`] self-alert.
///
/// `zones` is what Canopy managed to read from its configuration, and
/// `config_error` is why it read nothing when the configuration was present but
/// unparseable. The two carry different messages and different severity, because
/// one is a Canopy fault and the other is a tidy-up after a deliberate change.
///
/// A deployment with no zones configured and nothing claimed is not a problem:
/// that is the feature simply not in use.
// spec: DOM#when-the-zone-configuration-changes
pub async fn sweep_dns_zone_coverage(
	conn: &mut AsyncPgConnection,
	zones: &[ManagedZone],
	config_error: Option<&str>,
) -> Result<Option<Issue>> {
	let unzoned = crate::server_domains::ServerGroupDomain::unzoned(conn, zones).await?;

	if unzoned.is_empty() && config_error.is_none() {
		return recover(
			conn,
			DNS_ZONE_COVERAGE_REF,
			"every claimed group domain sits within a configured DNS zone",
		)
		.await;
	}

	let mut groups: Vec<&str> = unzoned.iter().map(|d| d.group_name.as_str()).collect();
	groups.sort_unstable();
	groups.dedup();
	let listed: Vec<String> = unzoned
		.iter()
		.map(|d| format!("{} ({})", d.domain, d.group_name))
		.collect();

	let (observed, message) = if let Some(error) = config_error {
		(
			CheckResult::Failed,
			format!(
				"Canopy's managed DNS zone configuration could not be read ({error}), so it is \
				 acting as though it has no zones. {} claimed domain(s) across {} group(s) are \
				 unusable until it is fixed{}",
				unzoned.len(),
				groups.len(),
				list_suffix(&listed),
			),
		)
	} else if zones.is_empty() {
		(
			CheckResult::Failed,
			format!(
				"Canopy has no managed DNS zones configured, but {} domain(s) across {} group(s) \
				 stand claimed{}",
				unzoned.len(),
				groups.len(),
				list_suffix(&listed),
			),
		)
	} else {
		(
			CheckResult::Warning,
			format!(
				"{} claimed domain(s) across {} group(s) fall outside every configured DNS zone \
				 ({}){}",
				unzoned.len(),
				groups.len(),
				zones
					.iter()
					.map(|z| z.apex.as_str())
					.collect::<Vec<_>>()
					.join(", "),
				list_suffix(&listed),
			),
		)
	};

	// The ceiling registers as `fail` whatever severity this raise carries, so a
	// warning that later becomes a fault isn't capped at warning by the policy
	// the first sighting seeded.
	raise(
		conn,
		DNS_ZONE_COVERAGE_REF,
		observed,
		CheckResult::Failed,
		false,
		Some(DNS_ZONE_COVERAGE_DOC),
		"Group domains outside Canopy's DNS zones",
		&message,
	)
	.await
	.map(Some)
}

/// `": a, b, c"`, or nothing at all for an empty list — a broken configuration
/// with no claims yet has nothing to name.
fn list_suffix(listed: &[String]) -> String {
	if listed.is_empty() {
		String::new()
	} else {
		format!(": {}", listed.join(", "))
	}
}

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
			scope: Scope::Global,
			device_id: None,
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
			scope: Scope::Global,
			device_id: None,
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
