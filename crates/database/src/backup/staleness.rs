//! Staleness scan over reported runs + maintenance-staleness, DB-driven.
//!
//! Server-centric, per `(server, type)`: the subject is the server being
//! protected; the device is the actor recorded in `backup_runs`.
//!
//! The scanned set is every enabled `(server, type)` capability whose
//! effective schedule has a non-NULL `expected_interval` and whose group's
//! `server_group_backup_config.status = 'ready'`. Disabled / manual-only
//! (NULL interval) / non-ready configs are simply not in the set, so
//! unauthorized or un-set-up devices never alert.

use std::collections::HashMap;

use commons_errors::Result;
use commons_types::{backup::BackupType, issue::Severity};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Span, SpanRelativeTo, SpanRound, Timestamp, Unit};
use uuid::Uuid;

use crate::{
	backup::{alerts::raise_group_event, refs},
	issues::NewEvent,
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

/// Build the scan set: every enabled `(server, type)` capability in a
/// `status='ready'` group whose effective schedule has a non-NULL
/// `expected_interval`. Per `(server, type)`, attach the latest backup success
/// and the `MIN(first_seen)` anchor.
pub async fn scan_rows(db: &mut AsyncPgConnection) -> Result<Vec<ScanRow>> {
	use crate::schema::{
		device_server_associations as dsa, server_backup_capabilities as cap,
		server_group_backup_config as cfg, server_group_backup_schedule as sched, servers,
	};

	// (server_id, group_id, is_monitored, device_id, type, expected_interval, config_created_at)
	// Join: servers -> their group's ready config -> enabled capability ->
	// per-(group,type) schedule with a non-NULL interval.
	let base: Vec<(
		Uuid,
		Uuid,
		bool,
		Option<Uuid>,
		String,
		crate::pg_duration::PgDuration,
		jiff_diesel::Timestamp,
	)> = servers::table
		.inner_join(cfg::table.on(cfg::group_id.nullable().eq(servers::group_id)))
		.inner_join(cap::table.on(cap::server_id.eq(servers::id)))
		.inner_join(
			sched::table.on(sched::group_id
				.eq(cfg::group_id)
				.and(sched::type_.eq(cap::type_))),
		)
		.filter(servers::deleted_at.is_null())
		.filter(cfg::status.eq("ready"))
		.filter(cap::enabled.eq(true))
		.filter(sched::expected_interval.is_not_null())
		.select((
			servers::id,
			cfg::group_id,
			servers::is_monitored,
			servers::device_id,
			cap::type_,
			sched::expected_interval.assume_not_null(),
			cfg::created_at,
		))
		.load(db)
		.await?;

	if base.is_empty() {
		return Ok(Vec::new());
	}

	// Anchors: MIN(first_seen) per server over all its association rows.
	let server_ids: Vec<Uuid> = {
		let mut s: std::collections::HashSet<Uuid> = base.iter().map(|r| r.0).collect();
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
		let mut s: std::collections::HashSet<Uuid> = base.iter().map(|r| r.1).collect();
		s.drain().collect()
	};
	let mut last_success: HashMap<(Uuid, BackupType), Timestamp> = HashMap::new();
	for gid in group_ids {
		let runs = crate::backups::BackupRun::latest_success_by_server_type_for_group(db, gid).await?;
		for ((sid, ty), run) in runs {
			last_success.insert((sid, ty), run.reported_at);
		}
	}

	Ok(base
		.into_iter()
		.map(
			|(server_id, group_id, is_monitored, device_id, ty, interval, created_at)| {
				let r#type = BackupType::from(ty);
				let last_success_at = last_success.get(&(server_id, r#type.clone())).copied();
				ScanRow {
					server_id,
					group_id,
					device_id,
					r#type,
					is_monitored,
					expected_interval: interval.0,
					config_created_at: Timestamp::from(created_at),
					min_first_seen: min_first_seen.get(&server_id).copied(),
					last_success_at,
				}
			},
		)
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
				NewEvent {
					source: refs::CANOPY_SOURCE.into(),
					r#ref: staleness_ref,
					severity: Some(Severity::Error),
					description: None,
					message: format!(
						"Server {label} has no successful {} backup newer than {} (last success {})",
						row.r#type,
						fmt_dur(grace),
						row.last_success_at
							.map(|t| t.to_string())
							.unwrap_or_else(|| "never".into()),
					),
					active: Some(true),
					occurred_at: Some(now),
				}
				.save(db, row.server_id, row.device_id)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Recovered => {
				NewEvent {
					source: refs::CANOPY_SOURCE.into(),
					r#ref: staleness_ref,
					severity: Some(Severity::Info),
					description: None,
					message: format!("Server {label} reported a successful {} backup again", row.r#type),
					active: Some(false),
					occurred_at: Some(now),
				}
				.save(db, row.server_id, row.device_id)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Never => {
				// `backup-never` clears on first success (it transitions to OK,
				// not Recovered — there's no recovery message for it). Only file
				// while it's still never-backed-up.
				NewEvent {
					source: refs::CANOPY_SOURCE.into(),
					r#ref: never_ref,
					severity: Some(Severity::Error),
					description: None,
					message: format!(
						"Server {label} has never reported a successful {} backup (expected since {})",
						row.r#type,
						row.anchor(),
					),
					active: Some(true),
					occurred_at: Some(now),
				}
				.save(db, row.server_id, row.device_id)
				.await?;
				filed += 1;
			}
			StalenessVerdict::Ok => {
				// If a `backup-never` is open but the server has now backed up,
				// clear it.
				if row.last_success_at.is_some()
					&& open_server_issue_active(db, row.server_id, &never_ref).await?
				{
					NewEvent {
						source: refs::CANOPY_SOURCE.into(),
						r#ref: never_ref,
						severity: Some(Severity::Info),
						description: None,
						message: format!("Server {label} reported its first successful {} backup", row.r#type),
						active: Some(false),
						occurred_at: Some(now),
					}
					.save(db, row.server_id, row.device_id)
					.await?;
					filed += 1;
				}
			}
		}
	}

	filed += sweep_maintenance(db, now).await?;
	Ok(filed)
}

/// Group-level maintenance staleness: a `status='ready'` group whose latest
/// successful maintenance run (any kind) is older than [`MAINTENANCE_STALE_AFTER`]
/// (or has none at all, past the threshold from its config creation) fires
/// `backup-maintenance-stale`; a fresh success clears it.
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
			raise_group_event(
				db,
				group_id,
				refs::MAINTENANCE_STALE,
				Severity::Error,
				None,
				&format!(
					"No successful repo maintenance for {} (last {})",
					fmt_dur(MAINTENANCE_STALE_AFTER),
					latest_success
						.map(|t| Timestamp::from(t).to_string())
						.unwrap_or_else(|| "never".into()),
				),
				true,
			)
			.await?;
			filed += 1;
		} else if !stale && was_active {
			raise_group_event(
				db,
				group_id,
				refs::MAINTENANCE_STALE,
				Severity::Info,
				None,
				"Repo maintenance completed successfully again",
				false,
			)
			.await?;
			filed += 1;
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
