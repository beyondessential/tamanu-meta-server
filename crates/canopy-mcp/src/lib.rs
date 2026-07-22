//! MCP (Model Context Protocol) query interface over the fleet.
//!
//! Spec: `.workhorse/specs/private-server/mcp.md` (id `MCP`).
//!
//! Mounted twice: at `/api/mcp` on the operator surface, behind the
//! tagged-device guard and an "any tailnet user" gate (private-server's
//! `mcp::require_tailnet_user`), and at `/mcp` on the internet-facing
//! surface behind the bearer-token gate (public-server's `mcp` module).
//! Every fleet tool only reads; nothing here mutates the fleet. The one
//! write surface is the manual incident record (`manual_incidents`
//! module), gated per caller by [`McpIdentity`].
//!
//! Tools call the existing `database` read functions directly and shape lean,
//! agent-legible JSON. The one piece of logic that must NOT be reimplemented is
//! backup staleness ("overdue" / "never reported"): `find_backup_problems` and
//! `fleet_summary` (in the `fleet` module) reuse [`database::backup::staleness`]
//! so the verdicts match what the operator UI and the alerting sweep present.
//!
//! Tools are grouped into domain modules (`servers`, `groups`, `versions`,
//! `fleet`, `backups`, `restore`, `incidents`, `manual_incidents`), each
//! contributing its own tool router (via rmcp's `#[tool_router(router = ...,
//! vis = "pub(crate)")]`) that [`CanopyMcp::new`] combines into the single
//! stored `ToolRouter`. `util` holds helpers shared across more than one of
//! those modules.

mod backups;
mod fleet;
mod groups;
mod incidents;
mod manual_incidents;
mod restore;
mod servers;
mod util;
mod versions;

use database::diesel_async::AsyncPgConnection;
use rmcp::{
	ServerHandler,
	handler::server::router::tool::ToolRouter,
	model::{ErrorData as McpError, ServerCapabilities, ServerInfo},
	tool_handler,
	transport::streamable_http_server::{
		StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
	},
};

/// The authenticated caller, inserted into the HTTP request's extensions by
/// each mount's auth gate. The transport carries the request parts into the
/// tool context, where the write tools read this to authorise and attribute
/// the write; the read tools never look at it.
#[derive(Clone, Debug)]
pub struct McpIdentity {
	/// The tailnet user's login (operator mount) or the token's name
	/// (internet-facing mount); recorded as the author of writes.
	pub who: String,
	/// Whether this caller may use the write tools: always for tailnet
	/// users, only for tokens minted with write access.
	pub can_write: bool,
}

#[derive(Clone)]
pub struct CanopyMcp {
	db_read: database::Db,
	db_write: database::Db,
	tool_router: ToolRouter<CanopyMcp>,
}

impl CanopyMcp {
	pub fn new(db_write: database::Db, db_read: database::Db) -> Self {
		Self {
			db_read,
			db_write,
			tool_router: Self::servers_router()
				+ Self::groups_router()
				+ Self::versions_router()
				+ Self::fleet_router()
				+ Self::backups_router()
				+ Self::restore_router()
				+ Self::incidents_router()
				+ Self::manual_incidents_router(),
		}
	}

	async fn conn(&self) -> Result<impl std::ops::DerefMut<Target = AsyncPgConnection>, McpError> {
		self.db_read.get().await.map_err(util::mcp_err)
	}

	/// A connection on the primary pool, for the manual-incident write
	/// tools; everything else reads via [`Self::conn`].
	async fn write_conn(
		&self,
	) -> Result<impl std::ops::DerefMut<Target = AsyncPgConnection>, McpError> {
		self.db_write.get().await.map_err(util::mcp_err)
	}
}

#[tool_handler(router = self.tool_router.clone())]
impl ServerHandler for CanopyMcp {
	fn get_info(&self) -> ServerInfo {
		let mut info = ServerInfo::default();
		info.instructions = Some(
			"Access to the Canopy fleet: servers, groups, health/status, Tamanu versions, \
			 backups, and incidents/issues. All data is live. Use find_* to locate entities and \
			 get_* for detail; fleet_summary and find_backup_problems for triage. Everything is \
			 read-only except manual incidents: support-recorded incident records \
			 (record/update/delete_manual_incident), which touch nothing else in the fleet. On \
			 the token-authenticated surface those write tools need a token minted with write \
			 access.\n\n\
			 Incidents: an incident groups the issues active for a group over a span of time. \
			 find_incidents returns everything open in the window, including heavy sub-grace \
			 flapping that was recorded but never surfaced. When summarizing or ranking, count \
			 `published` incidents (also given as `published_count`), not raw rows: an incident is \
			 published only if it outlived the group's grace window (~3 min default) or escalated. \
			 A count dominated by unpublished short-lived incidents usually means a twitchy \
			 threshold rather than a real outage."
				.into(),
		);
		info.capabilities = ServerCapabilities::builder().enable_tools().build();
		info
	}
}

// ---------------------------------------------------------------------------
// Service wiring.
// ---------------------------------------------------------------------------

/// Build the tower service nested into an axum router (`/api/mcp` on the
/// operator surface, `/mcp` on the internet-facing one). Auth is the mount's
/// business, not this service's: the mount's gate must insert an
/// [`McpIdentity`] into the request extensions for the write tools to work.
/// Reads go to `db_read`; only the manual-incident writes touch `db_write`.
pub fn service(
	db_write: database::Db,
	db_read: database::Db,
) -> StreamableHttpService<CanopyMcp, LocalSessionManager> {
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
		move || Ok(CanopyMcp::new(db_write.clone(), db_read.clone())),
		LocalSessionManager::default().into(),
		config,
	)
}
