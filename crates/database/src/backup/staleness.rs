//! Staleness scan over reported runs + maintenance-staleness, DB-driven.
//!
//! Server-centric, per `(server, type)`: the subject is the server being
//! protected; the device is the actor recorded in `backup_runs`.
//!
//! The scanned set is every enabled `(server, type)` capability whose
//! effective schedule has a non-NULL `expected_interval` and whose group's
//! `server_group_backup_config.status = 'ready'`. The effective schedule is
//! the group's override row if it has one, else the type's canopy-wide
//! `default_interval` — the same precedence the schedulers resolve with
//! ([`crate::backups::effective_interval`]), so every pair that is commanded
//! to back up is also monitored. Disabled / manual-only (an override row with
//! a NULL interval) / non-ready configs are simply not in the set, so
//! unauthorized or un-set-up devices never alert.

use std::collections::HashMap;

use commons_errors::Result;
use commons_types::{
	backup::{BackupType, RunOutcome},
	status::CheckResult,
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Span, SpanRelativeTo, SpanRound, Timestamp, Unit};
use uuid::Uuid;

use crate::{
	backup::refs,
	issues::{CheckFiling, Scope, file_check},
	servers::Server,
};

/// Maintenance cadence threshold. Independent of any backup `expected_interval`
/// — maintenance runs on a full-weekly cadence, so a group whose last
/// successful maintenance is older than this is stale regardless of how often
/// its backups run. ~8 days gives a one-day grace over the weekly cadence.
pub const MAINTENANCE_STALE_AFTER: SignedDuration = SignedDuration::from_hours(8 * 24);

/// One `(server, type)` to classify. Assembled by [`scan_rows`] from the
/// joined config / schedule / capability / runs / snapshots / associations.
#[derive(Debug, Clone)]
pub struct ScanRow {
	pub server_id: Uuid,
	pub group_id: Uuid,
	/// Latest-associated device, recorded on the `NewEvent` actor field.
	pub device_id: Option<Uuid>,
	pub r#type: BackupType,
	pub is_monitored: bool,
	pub expected_interval: SignedDuration,
	pub config_created_at: Timestamp,
	/// `MIN(first_seen)` over this server's `device_server_associations`.
	pub min_first_seen: Option<Timestamp>,
	/// Latest `purpose='backup' AND outcome='success'` for this `(server, type)`.
	pub last_success_at: Option<Timestamp>,
}

/// The staleness verdict for one `(server, type)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StalenessVerdict {
	/// A prior success exists but none newer than `now - grace`.
	Stale,
	/// No success ever, and `now - anchor > grace`.
	Never,
	/// Was stale, reporting success again (clears the issue).
	Recovered,
	/// Fresh enough, or not yet past the grace from its anchor — no alert.
	Ok,
}

impl ScanRow {
	/// `grace = expected_interval * 2`.
	fn grace(&self) -> SignedDuration {
		self.expected_interval.saturating_mul(2)
	}

	/// `anchor = max(min_first_seen, config_created_at)`. When the server has
	/// no associations (`min_first_seen` is NULL), the anchor degenerates to
	/// `config_created_at` alone.
	fn anchor(&self) -> Timestamp {
		match self.min_first_seen {
			Some(fs) if fs > self.config_created_at => fs,
			_ => self.config_created_at,
		}
	}

	/// Classify against `now`. `was_active` is whether a `backup-staleness`
	/// (or `backup-never`) issue is currently open for this `(server, type)` —
	/// used to distinguish a recovery from steady-state OK.
	pub fn classify(&self, now: Timestamp, was_active: bool) -> StalenessVerdict {
		let grace = self.grace();
		match self.last_success_at {
			Some(last) => {
				let stale = now.duration_since(last) > grace;
				if stale {
					StalenessVerdict::Stale
				} else if was_active {
					StalenessVerdict::Recovered
				} else {
					StalenessVerdict::Ok
				}
			}
			None => {
				// Never backed up: only alert once past the anchor + grace.
				if now.duration_since(self.anchor()) > grace {
					StalenessVerdict::Never
				} else {
					StalenessVerdict::Ok
				}
			}
		}
	}
}

/// Raw scan-set row: `(server_id, group_id, is_monitored, device_id, type,
/// has_schedule_override, override_interval, default_interval,
/// config_created_at)`.
type ScanBaseRow = (
	Uuid,
	Uuid,
	bool,
	Option<Uuid>,
	String,
	bool,
	Option<crate::pg_duration::PgDuration>,
	Option<crate::pg_duration::PgDuration>,
	jiff_diesel::Timestamp,
);

/// A scan-set row with its effective interval resolved, before the success and
/// anchor lookups are attached.
struct ScanRowBase {
	server_id: Uuid,
	group_id: Uuid,
	is_monitored: bool,
	device_id: Option<Uuid>,
	r#type: BackupType,
	expected_interval: SignedDuration,
	config_created_at: Timestamp,
}

/// Build the scan set: every enabled `(server, type)` capability in a
/// `status='ready'` group whose effective schedule has a non-NULL
/// `expected_interval`. Per `(server, type)`, attach the latest backup success
/// and the `MIN(first_seen)` anchor.
pub async fn scan_rows(db: &mut AsyncPgConnection) -> Result<Vec<ScanRow>> {
	use crate::schema::{
		backup_type_defaults as defaults, device_server_associations as dsa,
		server_backup_capabilities as cap, server_group_backup_config as cfg,
		server_group_backup_schedule as sched, servers,
	};

	// Join: servers -> their group's ready config -> enabled capability, with
	// both interval sources left-joined. The effective interval is resolved in
	// Rust below rather than in SQL, because "override row present but NULL"
	// (manual-only) and "no override row" (inherit the default) have to stay
	// distinguishable — a COALESCE would flatten them together.
	let base: Vec<ScanBaseRow> = servers::table
		.inner_join(cfg::table.on(cfg::group_id.nullable().eq(servers::group_id)))
		.inner_join(cap::table.on(cap::server_id.eq(servers::id)))
		.left_join(
			sched::table.on(sched::group_id
				.eq(cfg::group_id)
				.and(sched::type_.eq(cap::type_))),
		)
		.left_join(defaults::table.on(defaults::type_.eq(cap::type_)))
		.filter(servers::deleted_at.is_null())
		.filter(cfg::status.eq("ready"))
		.filter(cap::enabled.eq(true))
		.select((
			servers::id,
			cfg::group_id,
			servers::is_monitored,
			servers::device_id,
			cap::type_,
			sched::group_id.nullable().is_not_null(),
			sched::expected_interval.nullable(),
			defaults::default_interval.nullable(),
			cfg::created_at,
		))
		.load(db)
		.await?;

	// Resolve each pair's effective interval, dropping the ones with none: an
	// override row answers on its own (NULL = manual-only), otherwise the type
	// default applies. Mirrors [`crate::backups::effective_interval`].
	let base: Vec<ScanRowBase> = base
		.into_iter()
		.filter_map(
			|(
				server_id,
				group_id,
				is_monitored,
				device_id,
				ty,
				has_override,
				override_interval,
				default_interval,
				created_at,
			)| {
				let interval = if has_override {
					override_interval
				} else {
					default_interval
				}?;
				Some(ScanRowBase {
					server_id,
					group_id,
					is_monitored,
					device_id,
					r#type: BackupType::from(ty),
					expected_interval: interval.0,
					config_created_at: Timestamp::from(created_at),
				})
			},
		)
		.collect();

	if base.is_empty() {
		return Ok(Vec::new());
	}

	// Anchors: MIN(first_seen) per server over all its association rows.
	let server_ids: Vec<Uuid> = {
		let mut s: std::collections::HashSet<Uuid> = base.iter().map(|r| r.server_id).collect();
		s.drain().collect()
	};
	let assoc: Vec<(Uuid, Option<jiff_diesel::Timestamp>)> = dsa::table
		.group_by(dsa::server_id)
		.select((dsa::server_id, diesel::dsl::min(dsa::first_seen)))
		.filter(dsa::server_id.eq_any(&server_ids))
		.load(db)
		.await?;
	let min_first_seen: HashMap<Uuid, Timestamp> = assoc
		.into_iter()
		.filter_map(|(sid, ts)| ts.map(|t| (sid, Timestamp::from(t))))
		.collect();

	// Latest success per (server, type), fetched per distinct group.
	let group_ids: Vec<Uuid> = {
		let mut s: std::collections::HashSet<Uuid> = base.iter().map(|r| r.group_id).collect();
		s.drain().collect()
	};
	let mut last_success: HashMap<(Uuid, BackupType), Timestamp> = HashMap::new();
	for gid in group_ids {
		let runs =
			crate::backups::BackupRun::latest_success_by_server_type_for_group(db, gid).await?;
		for ((sid, ty), run) in runs {
			// The data's own moment, not when its upload finished: a backup that
			// took hours to transfer is as old as what it captured. `anchor` falls
			// back to the report time for a client that reports no freeze moment,
			// so this is a no-op for those. See `BackupRun::ANCHOR_SQL` for why the
			// query above orders by the same expression.
			last_success.insert((sid, ty), run.anchor());
		}
	}

	Ok(base
		.into_iter()
		.map(|row| {
			let last_success_at = last_success
				.get(&(row.server_id, row.r#type.clone()))
				.copied();
			ScanRow {
				server_id: row.server_id,
				group_id: row.group_id,
				device_id: row.device_id,
				r#type: row.r#type,
				is_monitored: row.is_monitored,
				expected_interval: row.expected_interval,
				config_created_at: row.config_created_at,
				min_first_seen: min_first_seen.get(&row.server_id).copied(),
				last_success_at,
			}
		})
		.collect())
}

fn type_suffix(ty: &BackupType) -> String {
	format!(":{ty}")
}

/// Run the full staleness sweep over a pre-computed scan: classify
/// every scanned `(server, type)` and file/clear the per-server
/// `backup-staleness` / `backup-never` issues, then the group-level
/// `backup-maintenance-stale`. Returns the number of events filed.
pub async fn sweep(db: &mut AsyncPgConnection, rows: &[ScanRow]) -> Result<usize> {
	let now = Timestamp::now();
	let mut filed = 0usize;

	for row in rows {
		// The ref is per-(server, type): the (server_id, source, ref) issue key
		// must distinguish types, so the type is folded into the ref suffix.
		let staleness_ref = format!("{}{}", refs::STALENESS, type_suffix(&row.r#type));
		let never_ref = format!("{}{}", refs::NEVER, type_suffix(&row.r#type));

		let was_active = open_server_issue_active(db, row.server_id, &staleness_ref).await?;
		let verdict = row.classify(now, was_active);

		let server = Server::get_by_id(db, row.server_id).await?;
		let label = server_label(&server);

		match verdict {
			StalenessVerdict::Stale => {
				let grace = row.grace();
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(row.server_id),
						device_id: row.device_id,
						check: &staleness_ref,
						observed: CheckResult::Failed,
						title: None,
						message: &format!(
							"Server {label} has no successful {} backup newer than {} (last success {})",
							row.r#type,
							fmt_dur(grace),
							row.last_success_at
								.map(|t| t.to_string())
								.unwrap_or_else(|| "never".into()),
						),
						detail: Some(serde_json::json!({
							"type": row.r#type.to_string(),
							"grace_secs": grace.as_secs(),
							"last_success_at": row.last_success_at.map(|t| t.to_string()),
						})),
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::STALENESS_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Recovered => {
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(row.server_id),
						device_id: row.device_id,
						check: &staleness_ref,
						observed: CheckResult::Passed,
						title: None,
						message: &format!(
							"Server {label} reported a successful {} backup again",
							row.r#type
						),
						detail: None,
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::STALENESS_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Never => {
				// `backup-never` clears on first success (it transitions to OK,
				// not Recovered — there's no recovery message for it). Only file
				// while it's still never-backed-up.
				//
				// Warning, not failure: a server that has *never* backed up
				// (freshly set up, or blocked by an upstream issue) shouldn't
				// open an incident. A *missed* backup — a server that was
				// backing up and stopped (`Stale`, above) — is the failure
				// that pages.
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(row.server_id),
						device_id: row.device_id,
						check: &never_ref,
						observed: CheckResult::Warning,
						title: None,
						message: &format!(
							"Server {label} has never reported a successful {} backup (expected since {})",
							row.r#type,
							row.anchor(),
						),
						detail: Some(serde_json::json!({
							"type": row.r#type.to_string(),
							"expected_since": row.anchor().to_string(),
						})),
						default_ceiling: CheckResult::Warning,
						default_escalates: false,
						documentation: Some(refs::NEVER_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Ok => {
				// If a `backup-never` is open but the server has now backed up,
				// clear it.
				if row.last_success_at.is_some()
					&& open_server_issue_active(db, row.server_id, &never_ref).await?
				{
					file_check(
						db,
						CheckFiling {
							source: crate::statuses::CANOPY_SOURCE,
							scope: Scope::Server(row.server_id),
							device_id: row.device_id,
							check: &never_ref,
							observed: CheckResult::Passed,
							title: None,
							message: &format!(
								"Server {label} reported its first successful {} backup",
								row.r#type
							),
							detail: None,
							default_ceiling: CheckResult::Warning,
							default_escalates: false,
							documentation: Some(refs::NEVER_DOC),
						},
					)
					.await?;
					filed += 1;
				}
			}
		}
	}

	filed += sweep_maintenance(db, now).await?;
	Ok(filed)
}

/// Group-level maintenance health, per `status='ready'` group:
///
/// - **Staleness** ([`refs::MAINTENANCE_STALE`]): latest *successful* run (any
///   kind) older than [`MAINTENANCE_STALE_AFTER`] — or none at all, past the
///   threshold from config creation. A fresh success clears it.
/// - **Failure** ([`refs::MAINTENANCE_ERROR`]): the most recently *finished*
///   run failed. Distinct from staleness — maintenance can run on cadence yet
///   error every time. A newer successful run clears it.
///
/// Both are `Error` severity (open an incident + page). A group that is both
/// stale and erroring files both, independently keyed.
async fn sweep_maintenance(db: &mut AsyncPgConnection, now: Timestamp) -> Result<usize> {
	use crate::schema::{backup_maintenance_runs as mr, server_group_backup_config as cfg};

	let groups: Vec<(Uuid, jiff_diesel::Timestamp)> = cfg::table
		.filter(cfg::status.eq("ready"))
		.select((cfg::group_id, cfg::created_at))
		.load(db)
		.await?;

	let mut filed = 0usize;
	for (group_id, created_at) in groups {
		let latest_success: Option<jiff_diesel::Timestamp> = mr::table
			.filter(mr::group_id.eq(group_id))
			.filter(mr::outcome.eq("success"))
			.select(diesel::dsl::max(mr::finished_at))
			.first::<Option<jiff_diesel::Timestamp>>(db)
			.await?;

		let reference = latest_success
			.map(Timestamp::from)
			.unwrap_or_else(|| Timestamp::from(created_at));
		let stale = now.duration_since(reference) > MAINTENANCE_STALE_AFTER;
		let was_active = open_group_issue_active(db, group_id, refs::MAINTENANCE_STALE).await?;

		if stale {
			file_check(
				db,
				CheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Group(group_id),
					device_id: None,
					check: refs::MAINTENANCE_STALE,
					observed: CheckResult::Failed,
					title: None,
					message: &format!(
						"No successful repo maintenance for {} (last {})",
						fmt_dur(MAINTENANCE_STALE_AFTER),
						latest_success
							.map(|t| Timestamp::from(t).to_string())
							.unwrap_or_else(|| "never".into()),
					),
					detail: Some(serde_json::json!({
						"threshold_secs": MAINTENANCE_STALE_AFTER.as_secs(),
						"last_success_at": latest_success.map(|t| Timestamp::from(t).to_string()),
					})),
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
					documentation: Some(refs::MAINTENANCE_STALE_DOC),
				},
			)
			.await?;
			filed += 1;
		} else if !stale && was_active {
			file_check(
				db,
				CheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Group(group_id),
					device_id: None,
					check: refs::MAINTENANCE_STALE,
					observed: CheckResult::Passed,
					title: None,
					message: "Repo maintenance completed successfully again",
					detail: None,
					default_ceiling: CheckResult::Failed,
					default_escalates: false,
					documentation: Some(refs::MAINTENANCE_STALE_DOC),
				},
			)
			.await?;
			filed += 1;
		}

		// Failure leg: a group can run maintenance on cadence yet error every
		// time, which staleness (absence-of-success) never catches. Key off the
		// most recently *finished* run.
		let latest_completed =
			crate::backups::BackupMaintenanceRun::latest_completed_for_group(db, group_id).await?;
		let err_active = open_group_issue_active(db, group_id, refs::MAINTENANCE_ERROR).await?;
		match latest_completed {
			Some(run) if run.outcome == Some(RunOutcome::Failure) => {
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Group(group_id),
						device_id: None,
						check: refs::MAINTENANCE_ERROR,
						observed: CheckResult::Failed,
						title: None,
						message: &format!(
							"Repo maintenance ({}) failed: {}",
							run.kind,
							run.error.as_deref().unwrap_or("(no detail reported)"),
						),
						detail: Some(serde_json::json!({
							"kind": run.kind,
							"error": run.error,
						})),
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::MAINTENANCE_ERROR_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			// Most recent finished run succeeded (or there is none): clear any
			// open failure issue.
			_ if err_active => {
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Group(group_id),
						device_id: None,
						check: refs::MAINTENANCE_ERROR,
						observed: CheckResult::Passed,
						title: None,
						message: "Repo maintenance completed successfully again",
						detail: None,
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::MAINTENANCE_ERROR_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			_ => {}
		}
	}
	Ok(filed)
}

/// Whether a server-scoped `(canopy, ref)` issue is currently open + active.
pub(crate) async fn open_server_issue_active(
	db: &mut AsyncPgConnection,
	server_id: Uuid,
	r#ref: &str,
) -> Result<bool> {
	use crate::schema::issues::dsl;
	let n: i64 = dsl::issues
		.filter(dsl::server_id.eq(server_id))
		.filter(dsl::source.eq(refs::CANOPY_SOURCE))
		.filter(dsl::ref_.eq(r#ref))
		.filter(dsl::active.eq(true))
		.filter(dsl::resolved_at.is_null())
		.count()
		.get_result(db)
		.await?;
	Ok(n > 0)
}

/// Whether a group-scoped `(canopy, ref)` issue is currently open + active.
pub(crate) async fn open_group_issue_active(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
	r#ref: &str,
) -> Result<bool> {
	use crate::schema::issues::dsl;
	let n: i64 = dsl::issues
		.filter(dsl::server_group_id.eq(group_id))
		.filter(dsl::source.eq(refs::CANOPY_SOURCE))
		.filter(dsl::ref_.eq(r#ref))
		.filter(dsl::active.eq(true))
		.filter(dsl::resolved_at.is_null())
		.count()
		.get_result(db)
		.await?;
	Ok(n > 0)
}

fn server_label(server: &Server) -> String {
	let host = server.host.as_ref().map(|h| h.0.to_string());
	match (&server.name, host) {
		(Some(n), Some(h)) if !n.is_empty() => format!("{n} ({h})"),
		(Some(n), None) if !n.is_empty() => n.clone(),
		(_, Some(h)) => h,
		(_, None) => server.id.to_string(),
	}
}

/// Render a threshold duration for alert messages via jiff's friendly
/// formatter, balanced to whole days. These are elapsed-time thresholds, not
/// calendar spans, so days are treated as invariant 24h
/// (`SpanRelativeTo::days_are_24_hours`) — no calendar anchor needed. Gives
/// "7d" / "12h" / "7d 12h", which reads better than `SignedDuration`'s
/// hours-capped "168h".
fn fmt_dur(d: SignedDuration) -> String {
	let span = Span::try_from(d)
		.and_then(|s| {
			s.round(
				SpanRound::new()
					.largest(Unit::Day)
					.relative(SpanRelativeTo::days_are_24_hours()),
			)
		})
		.unwrap_or_default();
	format!("{span:#}")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn fmt_dur_is_friendly_and_day_balanced() {
		assert_eq!(fmt_dur(SignedDuration::from_hours(7 * 24)), "7d");
		assert_eq!(fmt_dur(MAINTENANCE_STALE_AFTER), "8d");
		assert_eq!(fmt_dur(SignedDuration::from_hours(12)), "12h");
		assert_eq!(fmt_dur(SignedDuration::from_hours(7 * 24 + 12)), "7d 12h");
		assert_eq!(fmt_dur(SignedDuration::from_mins(30)), "30m");
	}
}
