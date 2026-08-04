//! Reconcile device **reports** against repo **inventory**: cross-check what
//! devices reported (`backup_runs`) against what *actually landed* in the repo
//! (`backup_repo_snapshots`).
//!
//! Per scanned `(server, type)`, resolved against the kopia source recorded in
//! `backup_repo_snapshots`:
//!
//! - **report success but no recent snapshot** → `backup-reconcile-missing`
//!   (`Error`, per-server). The case the reports alone cannot catch — a device
//!   lying about success or data not persisting. Detecting it needs the
//!   group's repo inventory, but the finding is about the one server whose
//!   report didn't hold up, so it is filed against that server: two servers in
//!   a group can fail it independently, and one recovering must not clear the
//!   other.
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
	backup::{
		refs,
		staleness::{ScanRow, label_list},
	},
	issues::{CheckInstance, InstancedCheckFiling, Scope, file_check_instances},
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

	// Inventory freshness is a property of the *group's* inspection, not of any
	// one pair's snapshot row: the inspection Job only writes a row for sources
	// it actually finds, so the pair with nothing in the repo — precisely the
	// one the "missing" verdict exists for — has no row of its own to date.
	let mut inspected_at: HashMap<Uuid, Timestamp> = HashMap::new();
	for gid in &group_ids {
		if let Some(at) =
			crate::backups::BackupRepoSnapshot::last_inspected_at_for_group(db, *gid).await?
		{
			inspected_at.insert(*gid, at);
		}
	}

	// How each scanned server is named in alert text, resolved in one query
	// rather than per finding.
	let labels: HashMap<Uuid, String> = {
		let ids: Vec<Uuid> = {
			let mut s: std::collections::HashSet<Uuid> = rows.iter().map(|r| r.server_id).collect();
			s.drain().collect()
		};
		Server::get_by_ids(db, &ids)
			.await?
			.iter()
			.map(|s| (s.id, crate::backup::staleness::server_label(s)))
			.collect()
	};

	// Latest comparable (reported, observed) sizes per (server, type). A server
	// belongs to one group, so keys don't collide across groups.
	let mut sized: HashMap<(Uuid, BackupType), (i64, i64)> = HashMap::new();
	for gid in &group_ids {
		sized.extend(
			crate::backups::BackupRun::latest_sized_by_server_type_for_group(db, *gid).await?,
		);
	}

	// One check per server, not one per backup type: group the scan by server
	// and grade each type as an instance (see the Names section of the CHK
	// spec).
	let mut by_server: HashMap<Uuid, Vec<&ScanRow>> = HashMap::new();
	let mut order: Vec<Uuid> = Vec::new();
	for row in rows {
		by_server
			.entry(row.server_id)
			.or_insert_with(|| {
				order.push(row.server_id);
				Vec::new()
			})
			.push(row);
	}

	let mut filed = 0usize;
	for server_id in order {
		let server_rows = &by_server[&server_id];
		let device_id = server_rows.iter().find_map(|r| r.device_id);
		// A scanned server always exists (the scan joins servers), so the
		// fallback is unreachable in practice — it just avoids a panic path.
		let label = labels
			.get(&server_id)
			.cloned()
			.unwrap_or_else(|| server_id.to_string());

		let missing_open = crate::backup::staleness::open_server_issue_active(
			db,
			server_id,
			refs::RECONCILE_MISSING,
		)
		.await?;
		let gap_open = crate::backup::staleness::open_server_issue_active(
			db,
			server_id,
			refs::RECONCILE_REPORT_GAP,
		)
		.await?;
		let size_open = crate::backup::staleness::open_server_issue_active(
			db,
			server_id,
			refs::RECONCILE_SIZE_MISMATCH,
		)
		.await?;

		let mut missing_instances: Vec<CheckInstance> = Vec::with_capacity(server_rows.len());
		let mut gap_instances: Vec<CheckInstance> = Vec::with_capacity(server_rows.len());
		let mut size_instances: Vec<CheckInstance> = Vec::with_capacity(server_rows.len());
		let mut any_missing = false;
		let mut any_decidable = false;
		let mut any_gap = false;
		let mut any_size = false;

		for row in server_rows {
			let grace = row.expected_interval.saturating_mul(2);
			let report_fresh = row
				.last_success_at
				.is_some_and(|t| now.duration_since(t) <= grace);
			let snap = snaps.get(&(row.server_id, row.r#type.clone()));
			let snapshot_fresh = snap
				.and_then(|s| s.latest_snapshot_at)
				.is_some_and(|t| now.duration_since(t) <= grace);
			let inventory_fresh = inspected_at
				.get(&row.group_id)
				.is_some_and(|at| now.duration_since(*at) <= INVENTORY_STALE_AFTER);

			// Report says success but the data didn't land. Only concluded when
			// the repo inventory is fresh enough to be trusted; a lagging
			// inspector must not produce false "report lied" findings, and a
			// type whose inventory is stale is left out of the check rather
			// than counted healthy.
			let missing = report_fresh && !snapshot_fresh && inventory_fresh;
			let missing_undecidable = report_fresh && !snapshot_fresh && !inventory_fresh;
			any_missing |= missing;
			any_decidable |= !missing_undecidable;
			missing_instances.push(CheckInstance {
				label: row.r#type.to_string(),
				observed: if missing {
					CheckResult::Failed
				} else if missing_undecidable {
					CheckResult::Skipped
				} else {
					CheckResult::Passed
				},
				detail: Some(serde_json::json!({
					"type": row.r#type.to_string(),
				})),
			});

			// Snapshot landed but no recent report → the reporting path is
			// broken, not the backup.
			let gap = !report_fresh && snapshot_fresh;
			any_gap |= gap;
			gap_instances.push(CheckInstance {
				label: row.r#type.to_string(),
				observed: if gap {
					CheckResult::Warning
				} else {
					CheckResult::Passed
				},
				detail: Some(serde_json::json!({
					"type": row.r#type.to_string(),
				})),
			});

			// Size discrepancy: orthogonal to freshness. Compare the latest run
			// that has both a reported and an observed size.
			let sizes = sized.get(&(row.server_id, row.r#type.clone()));
			let mismatch = sizes.is_some_and(|(reported, observed)| reported != observed);
			any_size |= mismatch;
			let mut size_detail = serde_json::Map::new();
			size_detail.insert("type".into(), row.r#type.to_string().into());
			if let Some(&(reported, observed)) = sizes {
				size_detail.insert("reported_bytes".into(), reported.into());
				size_detail.insert("observed_bytes".into(), observed.into());
			}
			size_instances.push(CheckInstance {
				label: row.r#type.to_string(),
				observed: if mismatch {
					CheckResult::Warning
				} else {
					CheckResult::Passed
				},
				detail: Some(serde_json::Value::Object(size_detail)),
			});
		}

		// A stale repo inventory makes the missing verdict undecidable, not
		// resolved: when it is stale for every type there is nothing to
		// conclude, so leave whatever state is already there rather than
		// clearing an open finding on the strength of a lagging inspector.
		if any_missing || (missing_open && any_decidable) {
			let total = missing_instances.len();
			file_check_instances(
				db,
				InstancedCheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Server(server_id),
					device_id,
					check: refs::RECONCILE_MISSING,
					title: None,
					instances: missing_instances,
					default_ceiling: CheckResult::Warning,
					default_escalates: false,
					documentation: Some(refs::RECONCILE_MISSING_DOC),
				},
				&|degraded| match degraded {
					[] => format!("Server {label} backup reports and repo snapshots agree again"),
					[one] => format!(
						"Server {label} reported a successful {} backup but no matching repo snapshot landed",
						one.label
					),
					many => format!(
						"Server {label} reported {} of its {total} backups successful with no matching repo snapshot: {}",
						many.len(),
						label_list(many),
					),
				},
			)
			.await?;
			filed += 1;
		}

		if any_gap || gap_open {
			let total = gap_instances.len();
			file_check_instances(
				db,
				InstancedCheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Server(server_id),
					device_id,
					check: refs::RECONCILE_REPORT_GAP,
					title: None,
					instances: gap_instances,
					default_ceiling: CheckResult::Warning,
					default_escalates: false,
					documentation: Some(refs::RECONCILE_REPORT_GAP_DOC),
				},
				&|degraded| match degraded {
					[] => format!("Backup reporting for {label} recovered"),
					[one] => format!(
						"A fresh {} repo snapshot exists for {label} but no backup run was reported",
						one.label
					),
					many => format!(
						"Fresh repo snapshots exist for {} of {label}'s {total} types with no backup run reported: {}",
						many.len(),
						label_list(many),
					),
				},
			)
			.await?;
			filed += 1;
		}

		if any_size || size_open {
			let total = size_instances.len();
			file_check_instances(
				db,
				InstancedCheckFiling {
					source: crate::statuses::CANOPY_SOURCE,
					scope: Scope::Server(server_id),
					device_id,
					check: refs::RECONCILE_SIZE_MISMATCH,
					title: None,
					instances: size_instances,
					default_ceiling: CheckResult::Warning,
					default_escalates: false,
					documentation: Some(refs::RECONCILE_SIZE_MISMATCH_DOC),
				},
				&|degraded| match degraded {
					[] => format!("Reported and repo snapshot sizes for {label} agree again"),
					[one] => format!(
						"Server {label} reported a {} snapshot size that disagrees with the repo",
						one.label
					),
					many => format!(
						"Server {label} reported snapshot sizes disagreeing with the repo for {} of its {total} types: {}",
						many.len(),
						label_list(many),
					),
				},
			)
			.await?;
			filed += 1;
		}
	}
	Ok(filed)
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
