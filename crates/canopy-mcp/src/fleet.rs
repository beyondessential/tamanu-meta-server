//! `fleet_summary` / `find_backup_problems` tools.

use std::collections::HashMap;

use commons_types::{
	Uuid,
	backup::{BackupConfigStatus, RunOutcome},
	status::{HealthState, ShortStatus},
};
use database::{
	backup::staleness::{StalenessVerdict, scan_rows},
	backups::{BackupMaintenanceRun, BackupRun, ServerGroupBackupConfig},
	server_groups::ServerGroup,
	servers::Server,
	statuses::Status,
};
use jiff::{SignedDuration, Timestamp};
use rmcp::{
	handler::server::wrapper::Parameters,
	model::{CallToolResult, ErrorData as McpError},
	tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
	CanopyMcp,
	util::{EmptyArgs, mcp_err, ok_json, parse_opt_uuid},
};

/// A run still open this long after it started is treated as stuck.
const STUCK_MAINTENANCE_AFTER: SignedDuration = SignedDuration::from_hours(6);
/// Only the last day of failed runs is surfaced, to bound noise.
const FAILED_RUN_WINDOW: SignedDuration = SignedDuration::from_hours(24);

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindBackupProblemsArgs {
	/// Narrow the scan to one group's id. Omit to scan the whole fleet.
	pub group_id: Option<String>,
}

#[derive(Serialize, Default)]
struct Counts {
	by_kind: HashMap<String, usize>,
	by_rank: HashMap<String, usize>,
}

#[derive(Serialize, Default)]
struct HealthRollup {
	healthy: usize,
	warning: usize,
	unhealthy: usize,
	unreachable: usize,
}

#[derive(Serialize, Default)]
struct BackupRollup {
	groups_configured: usize,
	groups_ready: usize,
	overdue: usize,
	never_reported: usize,
}

#[derive(Serialize)]
struct FleetSummary {
	total_servers: usize,
	groups: usize,
	counts: Counts,
	health: HealthRollup,
	version_distribution: HashMap<String, usize>,
	backups: BackupRollup,
}

#[derive(Serialize)]
struct BackupProblem {
	kind: &'static str,
	severity: &'static str,
	group_id: Uuid,
	#[serde(skip_serializing_if = "Option::is_none")]
	server_id: Option<Uuid>,
	#[serde(skip_serializing_if = "Option::is_none")]
	r#type: Option<String>,
	detail: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	since: Option<Timestamp>,
}

#[derive(Serialize)]
struct BackupProblems {
	count: usize,
	problems: Vec<BackupProblem>,
}

#[tool_router(router = fleet_router, vis = "pub(crate)")]
impl CanopyMcp {
	#[tool(
		description = "Fleet-wide overview: server counts by kind/rank, version distribution, a \
		               health rollup, and a backup-health rollup."
	)]
	async fn fleet_summary(
		&self,
		Parameters(_): Parameters<EmptyArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let servers = Server::get_all(&mut conn, 0, None).await.map_err(mcp_err)?;
		let ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;
		let st_by: HashMap<Uuid, &Status> = statuses.iter().map(|s| (s.server_id, s)).collect();
		let server_groups: Vec<(Uuid, Option<Uuid>)> =
			servers.iter().map(|s| (s.id, s.group_id)).collect();
		let state_health = database::issues::health_from_check_state(&mut conn, &server_groups)
			.await
			.map_err(mcp_err)?;

		let mut counts = Counts::default();
		let mut health = HealthRollup::default();
		let mut version_distribution: HashMap<String, usize> = HashMap::new();
		for s in &servers {
			*counts.by_kind.entry(s.kind.to_string()).or_default() += 1;
			if let Some(r) = &s.rank {
				*counts.by_rank.entry(r.to_string()).or_default() += 1;
			}
			let st = st_by.get(&s.id).copied();
			match st.map_or(ShortStatus::Gone, |s| s.short_status()) {
				ShortStatus::Down | ShortStatus::Gone => health.unreachable += 1,
				_ => match state_health.get(&s.id).copied().unwrap_or_default() {
					HealthState::Healthy => health.healthy += 1,
					HealthState::Warning => health.warning += 1,
					HealthState::Unhealthy => health.unhealthy += 1,
				},
			}
			if let Some(v) = st.and_then(|s| s.version.as_ref()) {
				*version_distribution.entry(v.to_string()).or_default() += 1;
			}
		}

		let groups = ServerGroup::list_all(&mut conn).await.map_err(mcp_err)?;
		let configs = ServerGroupBackupConfig::list(&mut conn)
			.await
			.map_err(mcp_err)?;
		let mut backups = BackupRollup {
			groups_configured: configs.len(),
			groups_ready: configs
				.iter()
				.filter(|c| c.status == BackupConfigStatus::Ready)
				.count(),
			..Default::default()
		};
		let now = Timestamp::now();
		for row in scan_rows(&mut conn).await.map_err(mcp_err)? {
			match row.classify(now, false) {
				StalenessVerdict::Stale => backups.overdue += 1,
				StalenessVerdict::Never => backups.never_reported += 1,
				_ => {}
			}
		}

		ok_json(&FleetSummary {
			total_servers: servers.len(),
			groups: groups.len(),
			counts,
			health,
			version_distribution,
			backups,
		})
	}

	#[tool(
		description = "Scan for current backup problems (fleet-wide, or one group): overdue and \
		               never-reported backups, provisioning errors, recent failed runs, and stuck \
		               maintenance. Each problem carries a severity."
	)]
	async fn find_backup_problems(
		&self,
		Parameters(args): Parameters<FindBackupProblemsArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let only_group = parse_opt_uuid(&args.group_id, "group_id")?;
		let now = Timestamp::now();
		let mut problems = Vec::new();

		// Overdue / never-reported, from the canonical staleness scan.
		for row in scan_rows(&mut conn).await.map_err(mcp_err)? {
			if only_group.is_some_and(|g| row.group_id != g) {
				continue;
			}
			match row.classify(now, false) {
				StalenessVerdict::Stale => problems.push(BackupProblem {
					kind: "overdue_backup",
					severity: "error",
					group_id: row.group_id,
					server_id: Some(row.server_id),
					r#type: Some(row.r#type.to_string()),
					detail: format!(
						"no successful {} backup within its grace window",
						row.r#type
					),
					since: row.last_success_at,
				}),
				StalenessVerdict::Never => problems.push(BackupProblem {
					kind: "never_backed_up",
					severity: "warning",
					group_id: row.group_id,
					server_id: Some(row.server_id),
					r#type: Some(row.r#type.to_string()),
					detail: format!("never reported a successful {} backup", row.r#type),
					since: None,
				}),
				_ => {}
			}
		}

		// Per-group: provisioning errors, recent failed runs, stuck maintenance.
		let configs = ServerGroupBackupConfig::list(&mut conn)
			.await
			.map_err(mcp_err)?;
		for c in &configs {
			if only_group.is_some_and(|g| c.group_id != g) {
				continue;
			}
			if let Some(err) = &c.last_init_error {
				problems.push(BackupProblem {
					kind: "provisioning_error",
					severity: "error",
					group_id: c.group_id,
					server_id: None,
					r#type: None,
					detail: err.clone(),
					since: None,
				});
			}
			for run in BackupRun::list_for_group(&mut conn, c.group_id, 20)
				.await
				.map_err(mcp_err)?
			{
				if run.outcome == RunOutcome::Failure
					&& now.duration_since(run.reported_at) <= FAILED_RUN_WINDOW
				{
					problems.push(BackupProblem {
						kind: "failed_run",
						severity: "warning",
						group_id: c.group_id,
						server_id: run.server_id,
						r#type: Some(run.r#type.to_string()),
						detail: run
							.error
							.clone()
							.unwrap_or_else(|| "backup run failed".into()),
						since: Some(run.reported_at),
					});
				}
			}
			for m in BackupMaintenanceRun::list_for_group(&mut conn, c.group_id, 5)
				.await
				.map_err(mcp_err)?
			{
				if m.finished_at.is_none()
					&& now.duration_since(m.started_at) > STUCK_MAINTENANCE_AFTER
				{
					problems.push(BackupProblem {
						kind: "stuck_maintenance",
						severity: "warning",
						group_id: c.group_id,
						server_id: None,
						r#type: None,
						detail: format!(
							"{} maintenance still running since {}",
							m.kind, m.started_at
						),
						since: Some(m.started_at),
					});
				}
			}
		}

		ok_json(&BackupProblems {
			count: problems.len(),
			problems,
		})
	}
}
