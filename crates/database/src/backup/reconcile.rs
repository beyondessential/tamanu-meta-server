//! Reconcile device **reports** against repo **inventory**: cross-check what
//! devices reported (`backup_runs`) against what *actually landed* in the repo
//! (`backup_repo_snapshots`).
//!
//! Per scanned `(server, type)`, resolved against the kopia source recorded in
//! `backup_repo_snapshots`:
//!
//! - **report success but no recent snapshot** → `backup-reconcile-missing`
//!   (`Error`, group-level, pages regardless of monitored). The case the
//!   reports alone cannot catch — a device lying about success or data not
//!   persisting.
//! - **recent snapshot but no report** → `backup-reconcile-report-gap`
//!   (`Warning`, per-server, non-paging). The reporting path is broken, not
//!   the backup.
//! - **neither** → genuinely stale; the staleness scan already owns it, emit
//!   nothing.
//!
//! Independently, per `(server, type)`, the latest run that carries both a
//! device-reported size and an inspection-observed size is compared:
//!
//! - **sizes disagree** (both non-zero) → `backup-reconcile-size-mismatch`
//!   (`Warning`, per-server, non-paging). The device reported a snapshot size
//!   that doesn't match what the repo holds.
//!
//! Freshness guard: if the repo inventory for the group is itself stale (its
//! `observed_at` older than [`INVENTORY_STALE_AFTER`]), the "missing" verdict
//! is skipped — a lagging inspector must not produce false "report lied"
//! alerts.

use std::collections::HashMap;

use commons_errors::Result;
use commons_types::{backup::BackupType, status::CheckResult};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

use crate::{
	backup::{refs, staleness::ScanRow},
	issues::{CheckFiling, Scope, file_check},
	servers::Server,
};

/// How old a group's repo snapshot inventory (`observed_at`) may be before
/// reconciliation refuses to conclude "missing". Tied to the inspection
/// cadence floor (weekly for manual-only groups) plus a day of grace. If the
/// inspector hasn't run recently we can't trust "no snapshot landed", so we
/// defer to the inspection Job's own failure surfacing instead.
pub const INVENTORY_STALE_AFTER: SignedDuration = SignedDuration::from_hours((7 + 1) * 24);

/// Snapshot freshness + observed-at for one `(server, type)`, from
/// `backup_repo_snapshots`.
#[derive(Debug, Clone, Copy)]
struct SnapInfo {
	latest_snapshot_at: Option<Timestamp>,
	observed_at: Timestamp,
}

/// Reconcile the scanned set against repo snapshots. Reuses the same
/// [`ScanRow`]s that [`crate::backup::staleness::scan_rows`] produced (the
/// caller passes them in so the staleness scan and reconciliation share one
/// pass). Returns the number of events filed.
pub async fn sweep(db: &mut AsyncPgConnection, rows: &[ScanRow]) -> Result<usize> {
	if rows.is_empty() {
		return Ok(0);
	}
	let now = Timestamp::now();

	// Snapshot info per (server_id, type) across the scanned groups.
	let group_ids: Vec<Uuid> = {
		let mut s: std::collections::HashSet<Uuid> = rows.iter().map(|r| r.group_id).collect();
		s.drain().collect()
	};
	let snaps = snapshot_info(db, &group_ids).await?;

	// Latest comparable (reported, observed) sizes per (server, type). A server
	// belongs to one group, so keys don't collide across groups.
	let mut sized: HashMap<(Uuid, BackupType), (i64, i64)> = HashMap::new();
	for gid in &group_ids {
		sized.extend(
			crate::backups::BackupRun::latest_sized_by_server_type_for_group(db, *gid).await?,
		);
	}

	let mut filed = 0usize;
	for row in rows {
		let grace = row.expected_interval.saturating_mul(2);
		let report_fresh = row
			.last_success_at
			.is_some_and(|t| now.duration_since(t) <= grace);

		let snap = snaps.get(&(row.server_id, row.r#type.clone()));
		let snapshot_fresh = snap
			.and_then(|s| s.latest_snapshot_at)
			.is_some_and(|t| now.duration_since(t) <= grace);
		let inventory_fresh =
			snap.is_some_and(|s| now.duration_since(s.observed_at) <= INVENTORY_STALE_AFTER);

		let missing_ref = format!("{}:{}", refs::RECONCILE_MISSING, row.r#type);
		let gap_ref = format!("{}:{}", refs::RECONCILE_REPORT_GAP, row.r#type);

		match (report_fresh, snapshot_fresh) {
			// Report says success but the data didn't land (or its row is
			// missing). Only conclude this when the repo inventory is fresh
			// enough.
			(true, false) if inventory_fresh => {
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Group(row.group_id),
						device_id: None,
						check: &missing_ref,
						observed: CheckResult::Failed,
						title: None,
						message: &format!(
							"Server {} reported a successful {} backup but no matching repo snapshot landed",
							row.server_id, row.r#type,
						),
						detail: Some(serde_json::json!({
							"type": row.r#type.to_string(),
							"server_id": row.server_id,
						})),
						default_ceiling: CheckResult::Failed,
						default_escalates: false,
						documentation: Some(refs::RECONCILE_MISSING_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			// Both agree it's fine → clear any open missing alert.
			(true, true) => {
				if open_group_active(db, row.group_id, &missing_ref).await? {
					file_check(
						db,
						CheckFiling {
							source: crate::statuses::CANOPY_SOURCE,
							scope: Scope::Group(row.group_id),
							device_id: None,
							check: &missing_ref,
							observed: CheckResult::Passed,
							title: None,
							message: &format!(
								"Server {} backup report and repo snapshot agree again",
								row.server_id
							),
							detail: None,
							default_ceiling: CheckResult::Failed,
							default_escalates: false,
							documentation: Some(refs::RECONCILE_MISSING_DOC),
						},
					)
					.await?;
					filed += 1;
				}
				filed += clear_report_gap(db, row, &gap_ref).await?;
			}
			// Snapshot landed but no recent report → the reporting path is
			// broken. Per-server warning (non-paging).
			(false, true) => {
				let server = Server::get_by_id(db, row.server_id).await?;
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(row.server_id),
						device_id: row.device_id,
						check: &gap_ref,
						observed: CheckResult::Warning,
						title: None,
						message: &format!(
							"A fresh {} repo snapshot exists for {} but no backup run was reported",
							row.r#type, server.id,
						),
						detail: Some(serde_json::json!({
							"type": row.r#type.to_string(),
						})),
						default_ceiling: CheckResult::Warning,
						default_escalates: false,
						documentation: Some(refs::RECONCILE_REPORT_GAP_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			// Neither fresh → the staleness scan owns it; clear stale reconcile
			// alerts.
			(false, false) => {
				filed += clear_report_gap(db, row, &gap_ref).await?;
			}
			// (true, false) but the repo inventory is stale: skip the missing
			// verdict.
			(true, false) => {}
		}

		// Size discrepancy: orthogonal to freshness. Compare the latest run that
		// has both a reported and an observed size.
		let size_ref = format!("{}:{}", refs::RECONCILE_SIZE_MISMATCH, row.r#type);
		match sized.get(&(row.server_id, row.r#type.clone())) {
			Some(&(reported, observed)) if reported != observed => {
				let server = Server::get_by_id(db, row.server_id).await?;
				file_check(
					db,
					CheckFiling {
						source: crate::statuses::CANOPY_SOURCE,
						scope: Scope::Server(row.server_id),
						device_id: row.device_id,
						check: &size_ref,
						observed: CheckResult::Warning,
						title: None,
						message: &format!(
							"Server {} reported a {} snapshot size of {reported} bytes but the repo holds {observed}",
							server.id, row.r#type,
						),
						detail: Some(serde_json::json!({
							"type": row.r#type.to_string(),
							"reported_bytes": reported,
							"observed_bytes": observed,
						})),
						default_ceiling: CheckResult::Warning,
						default_escalates: false,
						documentation: Some(refs::RECONCILE_SIZE_MISMATCH_DOC),
					},
				)
				.await?;
				filed += 1;
			}
			// Agree, or no comparable run → clear any open mismatch.
			_ => {
				filed += clear_size_mismatch(db, row, &size_ref).await?;
			}
		}
	}
	Ok(filed)
}

/// Clear an open per-server size-mismatch issue when the sizes agree again (or
/// there's no longer a comparable run to disagree).
async fn clear_size_mismatch(
	db: &mut AsyncPgConnection,
	row: &ScanRow,
	size_ref: &str,
) -> Result<usize> {
	if !crate::backup::staleness::open_server_issue_active(db, row.server_id, size_ref).await? {
		return Ok(0);
	}
	file_check(
		db,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(row.server_id),
			device_id: row.device_id,
			check: size_ref,
			observed: CheckResult::Passed,
			title: None,
			message: &format!(
				"Reported and repo {} snapshot sizes for {} agree again",
				row.r#type, row.server_id
			),
			detail: None,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::RECONCILE_SIZE_MISMATCH_DOC),
		},
	)
	.await?;
	Ok(1)
}

/// Clear an open per-server report-gap issue when the report path is healthy
/// again (or the pair is genuinely stale and the staleness scan owns it).
async fn clear_report_gap(
	db: &mut AsyncPgConnection,
	row: &ScanRow,
	gap_ref: &str,
) -> Result<usize> {
	if !crate::backup::staleness::open_server_issue_active(db, row.server_id, gap_ref).await? {
		return Ok(0);
	}
	file_check(
		db,
		CheckFiling {
			source: crate::statuses::CANOPY_SOURCE,
			scope: Scope::Server(row.server_id),
			device_id: row.device_id,
			check: gap_ref,
			observed: CheckResult::Passed,
			title: None,
			message: &format!(
				"Backup reporting for {} ({}) recovered",
				row.server_id, row.r#type
			),
			detail: None,
			default_ceiling: CheckResult::Warning,
			default_escalates: false,
			documentation: Some(refs::RECONCILE_REPORT_GAP_DOC),
		},
	)
	.await?;
	Ok(1)
}

async fn open_group_active(
	db: &mut AsyncPgConnection,
	group_id: Uuid,
	r#ref: &str,
) -> Result<bool> {
	crate::backup::staleness::open_group_issue_active(db, group_id, r#ref).await
}

/// Raw snapshot row: `(server_id, type, latest_snapshot_at, observed_at)`.
type SnapshotRow = (
	Option<Uuid>,
	Option<String>,
	Option<jiff_diesel::Timestamp>,
	jiff_diesel::Timestamp,
);

/// Latest snapshot + observed-at per `(server_id, type)` for the given groups.
/// Rows with a NULL `server_id` (sources we can't attribute to a server) are
/// skipped.
async fn snapshot_info(
	db: &mut AsyncPgConnection,
	group_ids: &[Uuid],
) -> Result<HashMap<(Uuid, BackupType), SnapInfo>> {
	use crate::schema::backup_repo_snapshots as s;

	if group_ids.is_empty() {
		return Ok(HashMap::new());
	}

	let rows: Vec<SnapshotRow> = s::table
		.filter(s::group_id.eq_any(group_ids))
		.filter(s::server_id.is_not_null())
		.select((
			s::server_id,
			s::type_,
			s::latest_snapshot_at,
			s::observed_at,
		))
		.load(db)
		.await?;

	let mut out: HashMap<(Uuid, BackupType), SnapInfo> = HashMap::new();
	for (server_id, ty, latest, observed) in rows {
		let (Some(server_id), Some(ty)) = (server_id, ty) else {
			continue;
		};
		let info = SnapInfo {
			latest_snapshot_at: latest.map(Timestamp::from),
			observed_at: Timestamp::from(observed),
		};
		out.entry((server_id, BackupType::from(ty)))
			.and_modify(|e| {
				// Keep the freshest snapshot row for the pair.
				if info.observed_at > e.observed_at {
					*e = info;
				}
			})
			.or_insert(info);
	}
	Ok(out)
}
