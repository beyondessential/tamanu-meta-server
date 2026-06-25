//! Read-only MCP (Model Context Protocol) query interface over the fleet.
//!
//! Spec: `.workhorse/specs/private-server/mcp.md` (id `MCP`).
//!
//! Mounted at `/api/mcp` on the operator surface, behind the tagged-device
//! guard and an "any tailnet user" gate (see [`require_tailnet_user`]). Every
//! tool only reads; nothing here mutates the fleet.
//!
//! Tools call the existing `database` read functions directly and shape lean,
//! agent-legible JSON. The one piece of logic that must NOT be reimplemented is
//! backup staleness ("overdue" / "never reported"): [`find_backup_problems`]
//! and [`fleet_summary`] reuse [`database::backup::staleness`] so the verdicts
//! match what the operator UI and the alerting sweep present.

use std::collections::HashMap;

use commons_types::{
	Uuid,
	backup::{BackupConfigStatus, BackupPlacement, BackupRepoMode, RunOutcome},
	server::{kind::ServerKind, rank::ServerRank},
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use database::{
	backup::staleness::{StalenessVerdict, scan_rows},
	backups::{
		BackupMaintenanceRun, BackupRepoSnapshot, BackupRepoStats, BackupRun,
		ServerBackupCapability, ServerGroupBackupConfig, ServerGroupBackupSchedule,
	},
	diesel_async::AsyncPgConnection,
	server_groups::ServerGroup,
	servers::Server,
	statuses::Status,
	version_known_issues::VersionKnownIssue,
	versions::Version,
};
use jiff::{SignedDuration, Timestamp};
use rmcp::{
	ServerHandler,
	handler::server::{router::tool::ToolRouter, wrapper::Parameters},
	model::{CallToolResult, Content, ErrorData as McpError, ServerCapabilities, ServerInfo},
	tool, tool_handler, tool_router,
	transport::streamable_http_server::{
		StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
	},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::state::AppState;

/// A run still open this long after it started is treated as stuck.
const STUCK_MAINTENANCE_AFTER: SignedDuration = SignedDuration::from_hours(6);
/// Only the last day of failed runs is surfaced, to bound noise.
const FAILED_RUN_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// Default cap on `find_servers` results.
const DEFAULT_SERVER_LIMIT: u64 = 200;

#[derive(Clone)]
pub struct CanopyMcp {
	state: AppState,
	tool_router: ToolRouter<CanopyMcp>,
}

// ---------------------------------------------------------------------------
// Tool argument types (JSON Schema is generated from these). Identifiers are
// taken as strings and parsed, so callers don't need a UUID schema.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindServersArgs {
	/// Free-text term matched against name, host, or id (case-insensitive).
	pub query: Option<String>,
	/// Filter by kind: `central`, `facility`, or `canopy`.
	pub kind: Option<String>,
	/// Filter by rank: `production`, `clone`, `demo`, `test`, or `dev`.
	pub rank: Option<String>,
	/// Filter to one group's id.
	pub group_id: Option<String>,
	/// Include archived (soft-deleted) servers. Defaults to false.
	pub include_archived: Option<bool>,
	/// Max results to return (default 200).
	pub limit: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ServerIdArgs {
	/// The server's id.
	pub server_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindGroupsArgs {
	/// Free-text term matched against group name or id. Omit to list all.
	pub query: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GroupIdArgs {
	/// The group's id.
	pub group_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListVersionsArgs {
	/// Include draft (unpublished) versions. Defaults to false.
	pub include_drafts: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct VersionArgs {
	/// The Tamanu version, e.g. `2.34.1`.
	pub version: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EmptyArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindBackupProblemsArgs {
	/// Narrow the scan to one group's id. Omit to scan the whole fleet.
	pub group_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Output types.
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct ServerSummary {
	id: Uuid,
	name: Option<String>,
	host: Option<String>,
	kind: ServerKind,
	rank: Option<ServerRank>,
	group_id: Option<Uuid>,
	group_name: Option<String>,
	is_monitored: bool,
	archived: bool,
	/// When the most recent status was received, if any.
	last_seen: Option<Timestamp>,
	/// Last known Tamanu version (retained even when long offline).
	version: Option<VersionStr>,
	reachability: ShortStatus,
	health: HealthState,
}

#[derive(Serialize)]
struct FindServersResult {
	total_matched: usize,
	returned: usize,
	truncated: bool,
	servers: Vec<ServerSummary>,
}

#[derive(Serialize)]
struct StatusOut {
	reported_at: Timestamp,
	version: Option<VersionStr>,
	health: HealthState,
	healthy: bool,
	reachability: ShortStatus,
	platform: Option<String>,
	postgres_version: Option<String>,
	/// Raw per-check breakdown from the status push.
	checks: serde_json::Value,
}

#[derive(Serialize)]
struct BackupCapabilityOut {
	r#type: String,
	enabled: bool,
	last_successful_backup_at: Option<Timestamp>,
	last_snapshot_id: Option<String>,
}

#[derive(Serialize)]
struct ServerDetail {
	id: Uuid,
	name: Option<String>,
	host: Option<String>,
	kind: ServerKind,
	rank: Option<ServerRank>,
	cloud: Option<bool>,
	is_monitored: bool,
	archived: bool,
	notes: String,
	registered_at: Option<Timestamp>,
	group_id: Option<Uuid>,
	group_name: Option<String>,
	sibling_count: usize,
	reachability: ShortStatus,
	health: HealthState,
	latest_status: Option<StatusOut>,
	backups: Vec<BackupCapabilityOut>,
}

#[derive(Serialize)]
struct GroupList {
	groups: Vec<GroupSummary>,
}

#[derive(Serialize)]
struct GroupSummary {
	id: Uuid,
	name: String,
	member_count: i64,
	effective_version: Option<VersionStr>,
	highest_rank: Option<ServerRank>,
	backup_config: Option<BackupConfigStatus>,
	last_backup_at: Option<Timestamp>,
}

#[derive(Serialize)]
struct BackupConfigOut {
	status: BackupConfigStatus,
	bucket: String,
	prefix: String,
	region: Option<String>,
	mode: BackupRepoMode,
	placement: BackupPlacement,
	last_init_error: Option<String>,
	created_at: Timestamp,
	updated_at: Timestamp,
}

#[derive(Serialize)]
struct GroupBackups {
	config: Option<BackupConfigOut>,
	schedules: Vec<ServerGroupBackupSchedule>,
	repo_stats: Option<BackupRepoStats>,
	recent_runs: Vec<BackupRun>,
	maintenance_runs: Vec<BackupMaintenanceRun>,
	repo_snapshots: Vec<BackupRepoSnapshot>,
	last_inspected_at: Option<Timestamp>,
}

#[derive(Serialize)]
struct GroupDetail {
	id: Uuid,
	name: String,
	notes: String,
	tags: commons_types::server::TagMap,
	effective_version: Option<VersionStr>,
	created_at: Timestamp,
	updated_at: Timestamp,
	members: Vec<ServerSummary>,
	backups: GroupBackups,
}

#[derive(Serialize)]
struct VersionList {
	versions: Vec<VersionSummary>,
}

#[derive(Serialize)]
struct VersionSummary {
	version: String,
	status: String,
	head_release_date: Option<Timestamp>,
	changelog_summary: String,
	/// Live servers currently reporting this version.
	adoption: u32,
}

#[derive(Serialize)]
struct ServerRef {
	id: Uuid,
	name: Option<String>,
}

#[derive(Serialize)]
struct VersionDetail {
	version: String,
	status: String,
	head_release_date: Option<Timestamp>,
	changelog: String,
	known_issues: Vec<VersionKnownIssue>,
	available_updates: Vec<String>,
	adoption_count: usize,
	adopting_servers: Vec<ServerRef>,
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

// ---------------------------------------------------------------------------
// Tools.
// ---------------------------------------------------------------------------

#[tool_router]
impl CanopyMcp {
	pub fn new(state: AppState) -> Self {
		Self {
			state,
			tool_router: Self::tool_router(),
		}
	}

	#[tool(
		description = "Find servers by name/host/id substring, optionally filtered by kind, rank, \
		               or group. Returns compact records with last-seen, version, and health."
	)]
	async fn find_servers(
		&self,
		Parameters(args): Parameters<FindServersArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let kind = parse_opt::<ServerKind>(&args.kind, "kind")?;
		let rank = parse_opt::<ServerRank>(&args.rank, "rank")?;
		let group = parse_opt_uuid(&args.group_id, "group_id")?;
		let limit = args.limit.unwrap_or(DEFAULT_SERVER_LIMIT) as usize;

		let mut servers = Server::get_all(&mut conn, 0, None).await.map_err(mcp_err)?;
		if args.include_archived.unwrap_or(false) {
			servers.extend(Server::list_archived(&mut conn).await.map_err(mcp_err)?);
		}

		let q = args.query.as_deref().map(str::to_lowercase);
		servers.retain(|s| {
			kind.as_ref().is_none_or(|k| &s.kind == k)
				&& rank.as_ref().is_none_or(|r| s.rank.as_ref() == Some(r))
				&& group
					.as_ref()
					.is_none_or(|g| s.group_id.as_ref() == Some(g))
				&& q.as_deref().is_none_or(|q| server_matches(s, q))
		});

		let total_matched = servers.len();
		let truncated = total_matched > limit;
		servers.truncate(limit);
		if truncated {
			tracing::info!(total_matched, limit, "find_servers result truncated");
		}

		let ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;
		let st_by: HashMap<Uuid, &Status> = statuses.iter().map(|s| (s.server_id, s)).collect();
		let group_names = Server::group_names_by_server_ids(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;

		let summaries: Vec<ServerSummary> = servers
			.iter()
			.map(|s| {
				summarize(
					s,
					st_by.get(&s.id).copied(),
					group_names.get(&s.id).cloned().flatten(),
				)
			})
			.collect();

		ok_json(&FindServersResult {
			total_matched,
			returned: summaries.len(),
			truncated,
			servers: summaries,
		})
	}

	#[tool(
		description = "Full detail for one server: fields, latest status (version, health, \
		               platform, postgres), owning group, sibling count, and backups."
	)]
	async fn get_server(
		&self,
		Parameters(args): Parameters<ServerIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.server_id, "server_id")?;

		let Ok(server) = Server::get_by_id(&mut conn, id).await else {
			return Ok(not_found(format!("no server with id {id}")));
		};

		let latest = Status::latest_for_server(&mut conn, id)
			.await
			.map_err(mcp_err)?;
		let last_versioned = Status::last_with_version_for_server(&mut conn, id)
			.await
			.map_err(mcp_err)?;
		let version = latest
			.as_ref()
			.and_then(|s| s.version.clone())
			.or_else(|| last_versioned.and_then(|s| s.version));

		let group = match server.group_id {
			Some(gid) => ServerGroup::get_by_id(&mut conn, gid).await.ok(),
			None => None,
		};
		let sibling_count = server.siblings(&mut conn).await.map_err(mcp_err)?.len();

		let caps = ServerBackupCapability::list_for_server(&mut conn, id)
			.await
			.map_err(mcp_err)?;
		let mut backups = Vec::with_capacity(caps.len());
		for cap in &caps {
			let last = BackupRun::latest_success_for_server(&mut conn, id, &cap.r#type)
				.await
				.map_err(mcp_err)?;
			backups.push(BackupCapabilityOut {
				r#type: cap.r#type.to_string(),
				enabled: cap.enabled,
				last_successful_backup_at: last.as_ref().map(|r| r.reported_at),
				last_snapshot_id: last.and_then(|r| r.snapshot_id),
			});
		}

		let latest_status = latest.as_ref().map(|s| StatusOut {
			reported_at: s.created_at,
			version: version.clone(),
			health: s.health_state(),
			healthy: s.healthy,
			reachability: s.short_status(),
			platform: s.platform(),
			postgres_version: s.postgres_version(),
			checks: s.health.clone(),
		});

		ok_json(&ServerDetail {
			id: server.id,
			name: server.name.clone(),
			host: server.host.as_ref().map(|h| h.0.to_string()),
			kind: server.kind,
			rank: server.rank,
			cloud: server.cloud,
			is_monitored: server.is_monitored,
			archived: server.deleted_at.is_some(),
			notes: server.notes.clone(),
			registered_at: server.registered_at,
			group_id: server.group_id,
			group_name: group.as_ref().map(|g| g.name.clone()),
			sibling_count,
			reachability: latest
				.as_ref()
				.map_or(ShortStatus::Gone, |s| s.short_status()),
			health: latest
				.as_ref()
				.map_or(HealthState::default(), |s| s.health_state()),
			latest_status,
			backups,
		})
	}

	#[tool(
		description = "Find server groups by name/id, with live member count, effective version, \
		               highest member rank, backup config state, and last-backup time."
	)]
	async fn find_groups(
		&self,
		Parameters(args): Parameters<FindGroupsArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let groups = match args.query.as_deref() {
			Some(q) if !q.is_empty() => ServerGroup::search(&mut conn, q).await.map_err(mcp_err)?,
			_ => ServerGroup::list_all(&mut conn).await.map_err(mcp_err)?,
		};
		let ids: Vec<Uuid> = groups.iter().map(|g| g.id).collect();
		let counts = ServerGroup::live_server_counts(&mut conn)
			.await
			.map_err(mcp_err)?;
		let ranks = ServerGroup::highest_member_ranks(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;
		let configs = ServerGroupBackupConfig::list(&mut conn)
			.await
			.map_err(mcp_err)?;
		let cfg_status: HashMap<Uuid, BackupConfigStatus> =
			configs.iter().map(|c| (c.group_id, c.status)).collect();

		let mut out = Vec::with_capacity(groups.len());
		for g in &groups {
			let last_backup_at = BackupRun::latest_backup_at_for_group(&mut conn, g.id)
				.await
				.map_err(mcp_err)?;
			out.push(GroupSummary {
				id: g.id,
				name: g.name.clone(),
				member_count: counts.get(&g.id).copied().unwrap_or(0),
				effective_version: g.effective_version.clone(),
				highest_rank: ranks.get(&g.id).cloned(),
				backup_config: cfg_status.get(&g.id).copied(),
				last_backup_at,
			});
		}
		ok_json(&GroupList { groups: out })
	}

	#[tool(
		description = "Full detail for one group: members (with version/health), backup config, \
		               schedules, repo stats, and recent backup/maintenance activity."
	)]
	async fn get_group(
		&self,
		Parameters(args): Parameters<GroupIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.group_id, "group_id")?;
		let Ok(group) = ServerGroup::get_by_id(&mut conn, id).await else {
			return Ok(not_found(format!("no group with id {id}")));
		};

		let members = group.list_servers(&mut conn).await.map_err(mcp_err)?;
		let mids: Vec<Uuid> = members.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(&mut conn, &mids)
			.await
			.map_err(mcp_err)?;
		let st_by: HashMap<Uuid, &Status> = statuses.iter().map(|s| (s.server_id, s)).collect();
		let member_summaries = members
			.iter()
			.map(|s| summarize(s, st_by.get(&s.id).copied(), Some(group.name.clone())))
			.collect();

		let config = ServerGroupBackupConfig::get(&mut conn, id)
			.await
			.map_err(mcp_err)?
			.map(|c| BackupConfigOut {
				status: c.status,
				bucket: c.bucket,
				prefix: c.prefix,
				region: c.region,
				mode: c.mode,
				placement: c.placement,
				last_init_error: c.last_init_error,
				created_at: c.created_at,
				updated_at: c.updated_at,
			});
		let schedules = ServerGroupBackupSchedule::list_for_group(&mut conn, id)
			.await
			.map_err(mcp_err)?;
		let repo_stats = BackupRepoStats::get(&mut conn, id).await.map_err(mcp_err)?;
		let recent_runs = BackupRun::list_for_group(&mut conn, id, 10)
			.await
			.map_err(mcp_err)?;
		let maintenance_runs = BackupMaintenanceRun::list_for_group(&mut conn, id, 5)
			.await
			.map_err(mcp_err)?;
		let repo_snapshots = BackupRepoSnapshot::list_for_group(&mut conn, id)
			.await
			.map_err(mcp_err)?;
		let last_inspected_at = BackupRepoSnapshot::last_inspected_at_for_group(&mut conn, id)
			.await
			.map_err(mcp_err)?;

		ok_json(&GroupDetail {
			id: group.id,
			name: group.name.clone(),
			notes: group.notes.clone(),
			tags: group.tags.clone(),
			effective_version: group.effective_version.clone(),
			created_at: group.created_at,
			updated_at: group.updated_at,
			members: member_summaries,
			backups: GroupBackups {
				config,
				schedules,
				repo_stats,
				recent_runs,
				maintenance_runs,
				repo_snapshots,
				last_inspected_at,
			},
		})
	}

	#[tool(
		description = "List known Tamanu versions with release date, changelog summary, and how \
		               many live servers currently run each."
	)]
	async fn list_versions(
		&self,
		Parameters(args): Parameters<ListVersionsArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let versions = if args.include_drafts.unwrap_or(false) {
			Version::get_all_including_drafts(&mut conn).await
		} else {
			Version::get_all(&mut conn).await
		}
		.map_err(mcp_err)?;

		let adoption = self.version_adoption(&mut conn).await?;

		let mut out = Vec::with_capacity(versions.len());
		for v in &versions {
			let vs = version_str(v);
			let head_release_date = match vs.parse::<VersionStr>() {
				Ok(p) => Version::get_head_release_date(&mut conn, p).await.ok(),
				Err(_) => None,
			};
			out.push(VersionSummary {
				version: vs.clone(),
				status: v.status.to_string(),
				head_release_date,
				changelog_summary: first_line(&v.changelog),
				adoption: adoption.get(&vs).copied().unwrap_or(0),
			});
		}
		ok_json(&VersionList { versions: out })
	}

	#[tool(
		description = "Detail for one Tamanu version: changelog, known issues, available updates, \
		               and which live servers run it."
	)]
	async fn get_version(
		&self,
		Parameters(args): Parameters<VersionArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let vs = args.version.parse::<VersionStr>().map_err(|_| {
			McpError::invalid_params(format!("invalid version: {}", args.version), None)
		})?;

		let Ok(version) = Version::get_by_version(&mut conn, vs.clone()).await else {
			return Ok(not_found(format!("no version {vs}")));
		};

		let known_issues =
			VersionKnownIssue::list_for_minor(&mut conn, version.major, version.minor)
				.await
				.map_err(mcp_err)?;
		let available_updates = Version::get_updates_for_version(&mut conn, vs.clone())
			.await
			.map_err(mcp_err)?
			.into_iter()
			.map(|u| format!("{}.{}.{}", u.major, u.minor, u.patch))
			.collect();
		let head_release_date = Version::get_head_release_date(&mut conn, vs.clone())
			.await
			.ok();

		// Adoption: live servers whose latest status reports this version.
		let servers = Server::get_all(&mut conn, 0, None).await.map_err(mcp_err)?;
		let ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;
		let target = vs.to_string();
		let on_version: std::collections::HashSet<Uuid> = statuses
			.iter()
			.filter(|s| s.version.as_ref().map(|v| v.to_string()) == Some(target.clone()))
			.map(|s| s.server_id)
			.collect();
		let adopting_servers: Vec<ServerRef> = servers
			.iter()
			.filter(|s| on_version.contains(&s.id))
			.map(|s| ServerRef {
				id: s.id,
				name: s.name.clone(),
			})
			.collect();

		ok_json(&VersionDetail {
			version: target,
			status: version.status.to_string(),
			head_release_date,
			changelog: version.changelog.clone(),
			known_issues,
			available_updates,
			adoption_count: adopting_servers.len(),
			adopting_servers,
		})
	}

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
				_ => match st.map_or(HealthState::default(), |s| s.health_state()) {
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

impl CanopyMcp {
	async fn conn(&self) -> Result<impl std::ops::DerefMut<Target = AsyncPgConnection>, McpError> {
		self.state.db.get().await.map_err(mcp_err)
	}

	/// Count of live servers reporting each version (by version string).
	async fn version_adoption(
		&self,
		conn: &mut AsyncPgConnection,
	) -> Result<HashMap<String, u32>, McpError> {
		let servers = Server::get_all(conn, 0, None).await.map_err(mcp_err)?;
		let ids: Vec<Uuid> = servers.iter().map(|s| s.id).collect();
		let statuses = Status::latest_for_servers(conn, &ids)
			.await
			.map_err(mcp_err)?;
		let mut adoption: HashMap<String, u32> = HashMap::new();
		for st in &statuses {
			if let Some(v) = &st.version {
				*adoption.entry(v.to_string()).or_default() += 1;
			}
		}
		Ok(adoption)
	}
}

#[tool_handler(router = self.tool_router.clone())]
impl ServerHandler for CanopyMcp {
	fn get_info(&self) -> ServerInfo {
		let mut info = ServerInfo::default();
		info.instructions = Some(
			"Read-only access to the Canopy fleet: servers, groups, health/status, Tamanu \
			 versions, and backups. All data is live. Use find_* to locate entities and get_* \
			 for detail; fleet_summary and find_backup_problems for triage."
				.into(),
		);
		info.capabilities = ServerCapabilities::builder().enable_tools().build();
		info
	}
}

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn summarize(s: &Server, st: Option<&Status>, group_name: Option<String>) -> ServerSummary {
	ServerSummary {
		id: s.id,
		name: s.name.clone(),
		host: s.host.as_ref().map(|h| h.0.to_string()),
		kind: s.kind,
		rank: s.rank,
		group_id: s.group_id,
		group_name,
		is_monitored: s.is_monitored,
		archived: s.deleted_at.is_some(),
		last_seen: st.map(|s| s.created_at),
		version: st.and_then(|s| s.version.clone()),
		reachability: st.map_or(ShortStatus::Gone, |s| s.short_status()),
		health: st.map_or(HealthState::default(), |s| s.health_state()),
	}
}

fn server_matches(s: &Server, q: &str) -> bool {
	s.name
		.as_deref()
		.is_some_and(|n| n.to_lowercase().contains(q))
		|| s.host
			.as_ref()
			.is_some_and(|h| h.0.to_string().to_lowercase().contains(q))
		|| s.id.to_string().contains(q)
}

fn version_str(v: &Version) -> String {
	format!("{}.{}.{}", v.major, v.minor, v.patch)
}

fn first_line(s: &str) -> String {
	s.lines().next().unwrap_or("").trim().to_string()
}

fn parse_opt<T: std::str::FromStr>(v: &Option<String>, field: &str) -> Result<Option<T>, McpError> {
	match v.as_deref() {
		Some(s) => s
			.parse::<T>()
			.map(Some)
			.map_err(|_| McpError::invalid_params(format!("invalid {field}: {s}"), None)),
		None => Ok(None),
	}
}

fn parse_uuid(s: &str, field: &str) -> Result<Uuid, McpError> {
	Uuid::parse_str(s).map_err(|_| McpError::invalid_params(format!("invalid {field}: {s}"), None))
}

fn parse_opt_uuid(v: &Option<String>, field: &str) -> Result<Option<Uuid>, McpError> {
	v.as_deref().map(|s| parse_uuid(s, field)).transpose()
}

/// Serialize a payload into a tool result, providing both the structured
/// content (for clients that read it) and a pretty-printed text fallback.
fn ok_json<T: Serialize>(value: &T) -> Result<CallToolResult, McpError> {
	let json = serde_json::to_value(value).map_err(mcp_err)?;
	let text = serde_json::to_string_pretty(&json).map_err(mcp_err)?;
	let mut result = CallToolResult::success(vec![Content::text(text)]);
	result.structured_content = Some(json);
	Ok(result)
}

/// A "ran successfully but found nothing" result the caller's client renders.
fn not_found(message: String) -> CallToolResult {
	CallToolResult::error(vec![Content::text(message)])
}

/// Map any internal/db error into an MCP protocol error.
fn mcp_err(e: impl std::fmt::Display) -> McpError {
	McpError::internal_error(e.to_string(), None)
}

// ---------------------------------------------------------------------------
// Service wiring.
// ---------------------------------------------------------------------------

/// Build the tower service nested into the axum router at `/api/mcp`.
pub fn service(state: AppState) -> StreamableHttpService<CanopyMcp, LocalSessionManager> {
	let mut config = StreamableHttpServerConfig::default();
	// rmcp's `allowed_hosts` defaults to loopback only — a DNS-rebinding defense
	// aimed at browser-facing localhost MCP servers. That threat doesn't apply
	// here: the endpoint is reachable only through the Tailscale ingress (which
	// injects the caller's identity), behind the tagged-device guard and the
	// tailnet-user gate, and serves no CORS headers, so a browser can't make a
	// cross-origin POST to it. Left as-is the loopback default 403s the real
	// deployment host, so disable it. An operator who wants to pin the Host
	// allowlist anyway can set CANOPY_MCP_ALLOWED_HOSTS to a comma-separated
	// list (e.g. `canopy.example.ts.net`); loopback stays allowed for dev.
	match std::env::var("CANOPY_MCP_ALLOWED_HOSTS") {
		Ok(list) if !list.trim().is_empty() => {
			config.allowed_hosts.extend(
				list.split(',')
					.map(str::trim)
					.filter(|s| !s.is_empty())
					.map(ToOwned::to_owned),
			);
		}
		_ => config = config.disable_allowed_hosts(),
	}

	StreamableHttpService::new(
		move || Ok(CanopyMcp::new(state.clone())),
		LocalSessionManager::default().into(),
		config,
	)
}

/// Gate the MCP mount on an authenticated tailnet user (any user, not only
/// admins). Reuses the operator surface's `TailscaleUser` extractor, so the
/// debug-build dev bypass applies in local dev and tests. The caller's login is
/// logged so each query is attributable.
pub async fn require_tailnet_user(
	req: axum::extract::Request,
	next: axum::middleware::Next,
) -> Result<axum::response::Response, commons_errors::AppError> {
	use axum::extract::FromRequestParts as _;
	use commons_servers::tailscale_auth::TailscaleUser;

	let (mut parts, body) = req.into_parts();
	let user = TailscaleUser::from_request_parts(&mut parts, &()).await?;
	tracing::info!(login = %user.login, "mcp request");
	let req = axum::extract::Request::from_parts(parts, body);
	Ok(next.run(req).await)
}
