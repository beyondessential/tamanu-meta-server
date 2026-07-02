use serde::{Deserialize, Serialize};
use utoipa_axum::router::OpenApiRouter;

pub mod admins;
pub mod backups;
pub mod bestool;
pub mod commons;
pub mod devices;
pub mod healthchecks;
pub mod incidents;
pub mod issues;
pub mod mcp_tokens;
pub mod restore_replicas;
pub mod self_alerts;
pub mod server_groups;
pub mod servers;
pub mod silenced_refs;
pub mod sql;
pub mod statuses;
pub mod versions;

/// Standard wrapper for paginated list responses. The total reflects the full
/// row count (not just the current page) so the frontend can render page
/// counts without a separate count fetch.
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Page<T> {
	pub items: Vec<T>,
	pub total: u64,
}

/// Generate a 4-word lowercase, hyphen-separated passphrase from the EFF large
/// wordlist (~52 bits of entropy), e.g. `correct-horse-battery-staple`. Used to
/// wrap secrets (enrollment tickets, provisioned device keys) that travel to an
/// operator out of band.
pub(crate) fn generate_passphrase() -> String {
	use chbs::{config::BasicConfig, prelude::*, probability::Probability, word::WordList};

	let config = BasicConfig {
		words: 4,
		word_provider: WordList::builtin_eff_large().sampler(),
		separator: "-".into(),
		capitalize_first: Probability::Never,
		capitalize_words: Probability::Never,
	};
	config.to_scheme().generate()
}

pub fn routes() -> OpenApiRouter<crate::state::AppState> {
	OpenApiRouter::new().nest(
		"/api",
		OpenApiRouter::new()
			.nest("/admins", admins::routes())
			.nest("/backups", backups::routes())
			.nest("/bestool", bestool::routes())
			.nest("/commons", commons::routes())
			.nest("/devices", devices::routes())
			.nest("/healthchecks", healthchecks::routes())
			.nest("/incidents", incidents::routes())
			.nest("/issues", issues::routes())
			.nest("/mcp_tokens", mcp_tokens::routes())
			.nest("/restore_replicas", restore_replicas::routes())
			.nest("/self_alerts", self_alerts::routes())
			.nest("/server_groups", server_groups::routes())
			.nest("/servers", servers::routes())
			.nest("/silenced_refs", silenced_refs::routes())
			.nest("/sql", sql::routes())
			.nest("/statuses", statuses::routes())
			.nest("/versions", versions::routes()),
	)
}
