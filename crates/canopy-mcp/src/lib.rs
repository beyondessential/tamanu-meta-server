//! Read-only MCP (Model Context Protocol) query interface over the fleet.
//!
//! Spec: `.workhorse/specs/private-server/mcp.md` (id `MCP`).
//!
//! Mounted twice: at `/api/mcp` on the operator surface, behind the
//! tagged-device guard and an "any tailnet user" gate (private-server's
//! `mcp::require_tailnet_user`), and at `/mcp` on the internet-facing
//! surface behind the bearer-token gate (public-server's `mcp` module).
//! Every tool only reads; nothing here mutates the fleet.
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
	issue::Severity,
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
	issues::{Event, Incident, Issue, IssueListFilters},
	server_groups::ServerGroup,
	servers::Server,
	slack_outbox::SlackOutbox,
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

/// A run still open this long after it started is treated as stuck.
const STUCK_MAINTENANCE_AFTER: SignedDuration = SignedDuration::from_hours(6);
/// Only the last day of failed runs is surfaced, to bound noise.
const FAILED_RUN_WINDOW: SignedDuration = SignedDuration::from_hours(24);
/// Default cap on `find_servers` results.
const DEFAULT_SERVER_LIMIT: u64 = 200;

#[derive(Clone)]
pub struct CanopyMcp {
	db: database::Db,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindIncidentsArgs {
	/// Look back this many days; returns incidents that were open at any point in
	/// the window (still open, or closed within it). Default 7.
	pub since_days: Option<u32>,
	/// Restrict to one group's id.
	pub group_id: Option<String>,
	/// Filter by status: `open` (not yet closed), `resolved` (operator-resolved),
	/// or `all` (default).
	pub status: Option<String>,
	/// Max incidents to return (default 100).
	pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IncidentIdArgs {
	/// The incident's id.
	pub incident_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FindIssuesArgs {
	/// Only currently-active, unresolved issues. Default true.
	pub active_only: Option<bool>,
	/// Filter to these severities: `critical`, `error`, `warning`, `info`, `debug`.
	pub severities: Option<Vec<String>>,
	/// Restrict to issues whose server is in this group's id.
	pub group_id: Option<String>,
	/// Restrict to one server's id.
	pub server_id: Option<String>,
	/// Only issues last seen within this many days.
	pub since_days: Option<u32>,
	/// Max issues to return (default 100).
	pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IssueIdArgs {
	/// The issue's id.
	pub issue_id: String,
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

#[derive(Serialize)]
struct IncidentSummary {
	id: Uuid,
	group_id: Uuid,
	group_name: Option<String>,
	/// `open` (not closed), `resolved` (operator-resolved), or `closed`.
	status: &'static str,
	opened_at: Timestamp,
	closed_at: Option<Timestamp>,
	resolved_at: Option<Timestamp>,
	resolved_by: Option<String>,
	resolved_reason: Option<String>,
	/// Whether the incident ever escalated (a critical issue joined).
	escalated: bool,
	/// Whether the incident actually surfaced to operators (its Slack open
	/// notice was delivered): it outlived the group's grace window, or it
	/// escalated. Incidents that flapped shut within the grace never published.
	/// Prefer counting `published` incidents over raw rows.
	published: bool,
	/// How long the incident was (or has been) open, in seconds.
	open_duration_secs: i64,
	issue_count: i64,
	/// Raw count of status events the incident accumulated. NOT a measure of
	/// duration or severity — a high count can be a sub-minute flap.
	event_count: i64,
}

#[derive(Serialize)]
struct IncidentList {
	count: usize,
	/// How many of `count` actually surfaced to operators (see `published`).
	published_count: usize,
	since: Timestamp,
	incidents: Vec<IncidentSummary>,
}

#[derive(Serialize)]
struct IncidentIssueOut {
	issue_id: Uuid,
	severity: Severity,
	source: String,
	r#ref: String,
	description: Option<String>,
	message: String,
	active: bool,
	server_id: Option<Uuid>,
	server_name: Option<String>,
	first_seen: Timestamp,
	last_seen: Timestamp,
	joined_at: Timestamp,
	/// None = still attached to the incident.
	left_at: Option<Timestamp>,
}

#[derive(Serialize)]
struct IncidentDetail {
	id: Uuid,
	group_id: Uuid,
	group_name: Option<String>,
	status: &'static str,
	opened_at: Timestamp,
	closed_at: Option<Timestamp>,
	resolved_at: Option<Timestamp>,
	resolved_by: Option<String>,
	resolved_reason: Option<String>,
	escalated_at: Option<Timestamp>,
	/// Whether the incident surfaced to operators (Slack open delivered).
	published: bool,
	open_duration_secs: i64,
	created_at: Timestamp,
	updated_at: Timestamp,
	issues: Vec<IncidentIssueOut>,
}

#[derive(Serialize)]
struct IssueSummary {
	id: Uuid,
	server_id: Option<Uuid>,
	server_name: Option<String>,
	group_id: Option<Uuid>,
	source: String,
	r#ref: String,
	severity: Severity,
	description: Option<String>,
	message: String,
	active: bool,
	first_seen: Timestamp,
	last_seen: Timestamp,
	resolved_at: Option<Timestamp>,
	snoozed_until: Option<Timestamp>,
}

#[derive(Serialize)]
struct IssueList {
	count: usize,
	issues: Vec<IssueSummary>,
}

#[derive(Serialize)]
struct EventOut {
	created_at: Timestamp,
	occurred_at: Option<Timestamp>,
	severity: Severity,
	description: Option<String>,
	message: String,
	active: bool,
	occurrences: i32,
	last_seen: Timestamp,
}

#[derive(Serialize)]
struct IncidentRefOut {
	incident_id: Uuid,
	opened_at: Timestamp,
	closed_at: Option<Timestamp>,
}

#[derive(Serialize)]
struct IssueDetail {
	id: Uuid,
	server_id: Option<Uuid>,
	server_name: Option<String>,
	group_id: Option<Uuid>,
	source: String,
	r#ref: String,
	severity: Severity,
	description: Option<String>,
	message: String,
	active: bool,
	first_seen: Timestamp,
	last_seen: Timestamp,
	resolved_at: Option<Timestamp>,
	resolved_by: Option<String>,
	resolved_reason: Option<String>,
	snoozed_until: Option<Timestamp>,
	recent_events: Vec<EventOut>,
	incidents: Vec<IncidentRefOut>,
}

// ---------------------------------------------------------------------------
// Tools.
// ---------------------------------------------------------------------------

#[tool_router]
impl CanopyMcp {
	pub fn new(db: database::Db) -> Self {
		Self {
			db,
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

	#[tool(
		description = "List incidents that were open at any point in a recent window (default last \
		               7 days), optionally for one group. Use this for 'incidents open in the past \
		               week'.\n\n\
		               IMPORTANT for summaries/ranking: count `published` incidents, not raw rows. \
		               The window includes a large volume of sub-grace flapping (health checks that \
		               recover/refire, alerts that self-clear in under a minute) that was recorded \
		               but never surfaced to anyone. An incident is `published` only if its Slack \
		               open notice was delivered: it stayed open past the group's grace window \
		               (slack_open_delay, ~3 min by default) OR it escalated (a critical issue \
		               joined, which bypasses the grace). `event_count` is raw status-event churn \
		               and does NOT track duration or severity — a high-event incident can be a \
		               sub-minute flap. A high count dominated by unpublished short-lived rows \
		               usually means a twitchy alert/health-check threshold, not a real outage. \
		               `published_count` gives the surfaced subset directly."
	)]
	async fn find_incidents(
		&self,
		Parameters(args): Parameters<FindIncidentsArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let since = since_from_days(args.since_days.unwrap_or(7));
		let group = parse_opt_uuid(&args.group_id, "group_id")?;
		let limit = args.limit.unwrap_or(100);
		let status = args.status.as_deref().unwrap_or("all");

		let incidents: Vec<Incident> = Incident::list_open_since(&mut conn, since, group, limit)
			.await
			.map_err(mcp_err)?
			.into_iter()
			.filter(|i| match status {
				"open" => i.closed_at.is_none(),
				"resolved" => i.resolved_at.is_some(),
				_ => true,
			})
			.collect();

		let group_names = group_names(
			&mut conn,
			&unique(incidents.iter().map(|i| i.server_group_id)),
		)
		.await?;
		let ids: Vec<Uuid> = incidents.iter().map(|i| i.id).collect();
		let stats = Incident::stats_for(&self.db, &ids).await.map_err(mcp_err)?;
		let published = SlackOutbox::delivered_open_ids(&mut conn, &ids)
			.await
			.map_err(mcp_err)?;

		let summaries: Vec<IncidentSummary> = incidents
			.iter()
			.map(|i| {
				let s = stats.get(&i.id);
				IncidentSummary {
					id: i.id,
					group_id: i.server_group_id,
					group_name: group_names.get(&i.server_group_id).cloned(),
					status: incident_status(i),
					opened_at: i.opened_at,
					closed_at: i.closed_at,
					resolved_at: i.resolved_at,
					resolved_by: i.resolved_by.clone(),
					resolved_reason: i.resolved_reason.clone(),
					escalated: i.escalated_at.is_some(),
					published: published.contains(&i.id),
					open_duration_secs: open_duration_secs(i),
					issue_count: s.map_or(0, |s| s.issue_count),
					event_count: s.map_or(0, |s| s.event_count),
				}
			})
			.collect();

		ok_json(&IncidentList {
			count: summaries.len(),
			published_count: summaries.iter().filter(|s| s.published).count(),
			since,
			incidents: summaries,
		})
	}

	#[tool(
		description = "Full detail for one incident: timing, status, and the issues attached to it \
		               (with their severities and messages)."
	)]
	async fn get_incident(
		&self,
		Parameters(args): Parameters<IncidentIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.incident_id, "incident_id")?;
		let Ok((incident, rows)) = Incident::get_with_issues(&mut conn, id).await else {
			return Ok(not_found(format!("no incident with id {id}")));
		};
		let group = ServerGroup::get_by_id(&mut conn, incident.server_group_id)
			.await
			.ok();
		let published = SlackOutbox::delivered_open_ids(&mut conn, &[incident.id])
			.await
			.map_err(mcp_err)?
			.contains(&incident.id);
		let names = Server::names_by_ids(
			&mut conn,
			&unique(rows.iter().filter_map(|(_, i)| i.server_id)),
		)
		.await
		.map_err(mcp_err)?;

		let issues = rows
			.iter()
			.map(|(link, iss)| IncidentIssueOut {
				issue_id: iss.id,
				severity: iss.severity,
				source: iss.source.clone(),
				r#ref: iss.r#ref.clone(),
				description: iss.description.clone(),
				message: iss.message.clone(),
				active: iss.active,
				server_id: iss.server_id,
				server_name: iss
					.server_id
					.and_then(|s| names.get(&s))
					.and_then(|(n, _)| n.clone()),
				first_seen: iss.first_seen,
				last_seen: iss.last_seen,
				joined_at: link.joined_at,
				left_at: link.left_at,
			})
			.collect();

		ok_json(&IncidentDetail {
			id: incident.id,
			group_id: incident.server_group_id,
			group_name: group.as_ref().map(|g| g.name.clone()),
			status: incident_status(&incident),
			opened_at: incident.opened_at,
			closed_at: incident.closed_at,
			resolved_at: incident.resolved_at,
			resolved_by: incident.resolved_by.clone(),
			resolved_reason: incident.resolved_reason.clone(),
			escalated_at: incident.escalated_at,
			published,
			open_duration_secs: open_duration_secs(&incident),
			created_at: incident.created_at,
			updated_at: incident.updated_at,
			issues,
		})
	}

	#[tool(
		description = "List issues across the fleet, filtered by active state, severity, group, \
		               server, and recency. Issues are the per-(server,source,ref) events that make \
		               up incidents."
	)]
	async fn find_issues(
		&self,
		Parameters(args): Parameters<FindIssuesArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let severities = parse_severities(&args.severities)?;
		let group = parse_opt_uuid(&args.group_id, "group_id")?;
		let server = parse_opt_uuid(&args.server_id, "server_id")?;
		let since = args.since_days.map(since_from_days);
		let limit = args.limit.unwrap_or(100);

		let mut issues = Issue::list(
			&mut conn,
			IssueListFilters {
				active_only: args.active_only.unwrap_or(true),
				severities,
				server_group_id: group,
				since,
			},
			limit,
		)
		.await
		.map_err(mcp_err)?;
		if let Some(sid) = server {
			issues.retain(|i| i.server_id == Some(sid));
		}

		let names = Server::names_by_ids(
			&mut conn,
			&unique(issues.iter().filter_map(|i| i.server_id)),
		)
		.await
		.map_err(mcp_err)?;
		let summaries: Vec<IssueSummary> =
			issues.iter().map(|i| issue_summary(i, &names)).collect();
		ok_json(&IssueList {
			count: summaries.len(),
			issues: summaries,
		})
	}

	#[tool(
		description = "Full detail for one issue: its fields, recent events, and the incidents it \
		               is or was part of."
	)]
	async fn get_issue(
		&self,
		Parameters(args): Parameters<IssueIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.issue_id, "issue_id")?;
		let Ok(issue) = Issue::get_by_id(&mut conn, id).await else {
			return Ok(not_found(format!("no issue with id {id}")));
		};
		let events = Event::list_for_issue(&mut conn, id, 0, 20)
			.await
			.map_err(mcp_err)?;
		let inc = Incident::for_issues(&mut conn, &[id])
			.await
			.map_err(mcp_err)?;
		let server_name = match issue.server_id {
			Some(sid) => Server::names_by_ids(&mut conn, &[sid])
				.await
				.map_err(mcp_err)?
				.get(&sid)
				.and_then(|(n, _)| n.clone()),
			None => None,
		};

		let recent_events = events
			.iter()
			.map(|e| EventOut {
				created_at: e.created_at,
				occurred_at: e.occurred_at,
				severity: e.severity,
				description: e.description.clone(),
				message: e.message.clone(),
				active: e.active,
				occurrences: e.occurrences,
				last_seen: e.last_seen,
			})
			.collect();
		let incidents = inc
			.get(&id)
			.into_iter()
			.flatten()
			.map(|r| IncidentRefOut {
				incident_id: r.incident_id,
				opened_at: r.opened_at,
				closed_at: r.closed_at,
			})
			.collect();

		ok_json(&IssueDetail {
			id: issue.id,
			server_id: issue.server_id,
			server_name,
			group_id: issue.server_group_id,
			source: issue.source.clone(),
			r#ref: issue.r#ref.clone(),
			severity: issue.severity,
			description: issue.description.clone(),
			message: issue.message.clone(),
			active: issue.active,
			first_seen: issue.first_seen,
			last_seen: issue.last_seen,
			resolved_at: issue.resolved_at,
			resolved_by: issue.resolved_by.clone(),
			resolved_reason: issue.resolved_reason.clone(),
			snoozed_until: issue.snoozed_until,
			recent_events,
			incidents,
		})
	}
}

impl CanopyMcp {
	async fn conn(&self) -> Result<impl std::ops::DerefMut<Target = AsyncPgConnection>, McpError> {
		self.db.get().await.map_err(mcp_err)
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
			 versions, backups, and incidents/issues. All data is live. Use find_* to locate \
			 entities and get_* for detail; fleet_summary and find_backup_problems for triage.\n\n\
			 Incidents: an incident groups the issues active for a group over a span of time. \
			 find_incidents returns everything open in the window, including heavy sub-grace \
			 flapping that was recorded but never surfaced. When summarizing or ranking, count \
			 `published` incidents (also given as `published_count`), not raw rows: an incident is \
			 published only if it outlived the group's grace window (~3 min default) or escalated. \
			 `event_count` is raw event churn, not duration or severity — a huge count can be a \
			 sub-minute flap, usually a twitchy threshold rather than a real outage."
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

/// A timestamp `days` ago (clamped to a decade), for recency windows.
fn since_from_days(days: u32) -> Timestamp {
	let days = days.min(3650) as i64;
	Timestamp::now() - SignedDuration::from_hours(24 * days)
}

/// How long the incident was (or has been) open, in seconds.
fn open_duration_secs(i: &Incident) -> i64 {
	let end = i.closed_at.unwrap_or_else(Timestamp::now);
	end.duration_since(i.opened_at).as_secs().max(0)
}

fn incident_status(i: &Incident) -> &'static str {
	if i.resolved_at.is_some() {
		"resolved"
	} else if i.closed_at.is_some() {
		"closed"
	} else {
		"open"
	}
}

fn parse_severities(v: &Option<Vec<String>>) -> Result<Option<Vec<Severity>>, McpError> {
	match v {
		Some(list) if !list.is_empty() => {
			let mut out = Vec::with_capacity(list.len());
			for s in list {
				out.push(s.parse::<Severity>().map_err(|_| {
					McpError::invalid_params(format!("invalid severity: {s}"), None)
				})?);
			}
			Ok(Some(out))
		}
		_ => Ok(None),
	}
}

async fn group_names(
	conn: &mut AsyncPgConnection,
	ids: &[Uuid],
) -> Result<HashMap<Uuid, String>, McpError> {
	let groups = ServerGroup::list_by_ids(conn, ids).await.map_err(mcp_err)?;
	Ok(groups.into_iter().map(|g| (g.id, g.name)).collect())
}

/// Deduplicate a stream of ids, preserving first-seen order.
fn unique(it: impl IntoIterator<Item = Uuid>) -> Vec<Uuid> {
	let mut seen = std::collections::HashSet::new();
	it.into_iter().filter(|x| seen.insert(*x)).collect()
}

fn issue_summary(
	i: &Issue,
	names: &HashMap<Uuid, (Option<String>, Option<String>)>,
) -> IssueSummary {
	IssueSummary {
		id: i.id,
		server_id: i.server_id,
		server_name: i
			.server_id
			.and_then(|s| names.get(&s))
			.and_then(|(n, _)| n.clone()),
		group_id: i.server_group_id,
		source: i.source.clone(),
		r#ref: i.r#ref.clone(),
		severity: i.severity,
		description: i.description.clone(),
		message: i.message.clone(),
		active: i.active,
		first_seen: i.first_seen,
		last_seen: i.last_seen,
		resolved_at: i.resolved_at,
		snoozed_until: i.snoozed_until,
	}
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

/// Build the tower service nested into an axum router (`/api/mcp` on the
/// operator surface, `/mcp` on the internet-facing one). Auth is the mount's
/// business, not this service's.
pub fn service(db: database::Db) -> StreamableHttpService<CanopyMcp, LocalSessionManager> {
	let mut config = StreamableHttpServerConfig::default();
	// Stateless: each request is self-contained, with no server-side session.
	// The default stateful mode keeps sessions in process memory and 404s
	// ("Session not found") any follow-up request that a load balancer routes to
	// a different replica than the one that handled `initialize` — which is
	// exactly what a multi-replica deployment behind the Tailscale ingress does.
	// This is a read-only request/response API with no server-initiated push, so
	// sessions buy us nothing.
	config.stateful_mode = false;
	// Return plain `application/json` per request instead of an SSE stream. With
	// no streaming there's no long-lived response for a proxy to buffer or drop.
	config.json_response = true;
	// rmcp's `allowed_hosts` defaults to loopback only — a DNS-rebinding defense
	// aimed at browser-facing localhost MCP servers. That threat doesn't apply
	// here: both mounts sit behind an authenticating gate (tailnet identity on
	// the operator surface, bearer tokens on the internet-facing one) and serve
	// no CORS headers, so a browser can't make a credentialed cross-origin POST.
	// Left as-is the loopback default 403s the real deployment hosts, so disable
	// it. An operator who wants to pin the Host allowlist anyway can set
	// CANOPY_MCP_ALLOWED_HOSTS to a comma-separated list (e.g.
	// `canopy.example.ts.net`); loopback stays allowed for dev.
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
		move || Ok(CanopyMcp::new(db.clone())),
		LocalSessionManager::default().into(),
		config,
	)
}
