//! `find_servers` / `get_server` tools, and the [`ServerSummary`] shape
//! reused by the groups module for member listings.

use std::collections::HashMap;

use commons_types::{
	Uuid,
	server::{kind::ServerKind, rank::ServerRank},
	status::{HealthState, ShortStatus},
	version::VersionStr,
};
use database::{
	backups::BackupRun, backups::ServerBackupCapability, reported_detail::ReportedDetail,
	server_groups::ServerGroup, servers::Server, statuses::Status,
};
use jiff::Timestamp;
use rmcp::{
	handler::server::wrapper::Parameters,
	model::{CallToolResult, ErrorData as McpError},
	tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
	CanopyMcp,
	util::{mcp_err, not_found, ok_json, parse_opt, parse_opt_uuid, parse_uuid},
};

/// Default cap on `find_servers` results.
const DEFAULT_SERVER_LIMIT: u64 = 200;

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

#[derive(Serialize)]
pub(crate) struct ServerSummary {
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
	/// Raw per-check breakdown from the status push.
	checks: serde_json::Value,
}

/// What the server currently reports about the software it runs. Resolved
/// across every source reporting on it — a field is the most recent value any
/// source reported, so a field the latest push didn't carry is still here.
// spec: FIG#sourcing
#[derive(Serialize)]
struct FiguresOut {
	platform: Option<String>,
	postgres_version: Option<String>,
	node_version: Option<String>,
	bestool_version: Option<String>,
	timezone: Option<String>,
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
	/// What the server runs, as currently reported. Distinct from
	/// `latest_status`, which is one push and its own metadata.
	figures: FiguresOut,
	latest_status: Option<StatusOut>,
	backups: Vec<BackupCapabilityOut>,
}

#[tool_router(router = servers_router, vis = "pub(crate)")]
impl CanopyMcp {
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
		let server_groups: Vec<(Uuid, Option<Uuid>)> =
			servers.iter().map(|s| (s.id, s.group_id)).collect();
		let health = database::issues::health_from_check_state(&mut conn, &server_groups)
			.await
			.map_err(mcp_err)?;

		let summaries: Vec<ServerSummary> = servers
			.iter()
			.map(|s| {
				summarize(
					s,
					st_by.get(&s.id).copied(),
					group_names.get(&s.id).cloned().flatten(),
					health.get(&s.id).copied().unwrap_or_default(),
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
		// The last version any source reported, which already prefers the
		// newest report — no need to check the latest push separately, and it
		// holds for a server that's been down for months.
		let version = ReportedDetail::last_version(&mut conn, id)
			.await
			.map_err(mcp_err)?;

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

		// Health rolls up current check state across every source
		// (silenced checks skipped in the rollup); `checks` still carries
		// the latest push's raw results verbatim.
		let health =
			database::issues::health_from_check_state(&mut conn, &[(server.id, server.group_id)])
				.await
				.map_err(mcp_err)?
				.get(&server.id)
				.copied()
				.unwrap_or_default();

		let latest_status = latest.as_ref().map(|s| StatusOut {
			reported_at: s.created_at,
			version: version.clone(),
			health,
			healthy: s.healthy,
			reachability: s.short_status(),
			checks: s.health.clone(),
		});

		// Resolved across every source, not read off the latest push: sources
		// don't all carry the same fields, so a legacy heartbeat landing last
		// would otherwise blank out what the reporting agent said.
		// spec: FIG#sourcing
		let merged = ReportedDetail::merge(
			&ReportedDetail::for_server(&mut conn, server.id)
				.await
				.map_err(mcp_err)?,
		);
		let figures = FiguresOut {
			platform: merged.platform(),
			postgres_version: merged.postgres_version(),
			node_version: merged.node_version(),
			bestool_version: merged.bestool_version(),
			timezone: merged.timezone(),
		};

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
			health,
			figures,
			latest_status,
			backups,
		})
	}
}

/// Shared with the groups module, for member listings on [`crate::groups::GroupDetail`].
/// `health` is the server's check-state rollup (silenced checks already
/// skipped).
pub(crate) fn summarize(
	s: &Server,
	st: Option<&Status>,
	group_name: Option<String>,
	health: HealthState,
) -> ServerSummary {
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
		health,
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
