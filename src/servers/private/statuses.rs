use std::{collections::BTreeSet, sync::Arc};

use axum::{
	extract::State,
	response::Html,
	routing::{Router, get, post},
};
use serde::Serialize;
use tera::{Context, Tera};

use crate::{
	db::{latest_statuses::LatestStatus, server_rank::ServerRank, statuses::Status},
	error::Result,
	servers::version::Version,
	state::{AppState, Db},
};

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
pub struct LiveVersionsBracket {
	pub min: Version,
	pub max: Version,
}

async fn view(State(db): State<Db>, State(tera): State<Arc<Tera>>) -> Result<Html<String>> {
	let mut db = db.get().await?;
	let entries = LatestStatus::fetch(&mut db).await?;

	let versions = entries
		.iter()
		.filter_map(|status| {
			if let (Some(version), true, ServerRank::Production) = (
				status.latest_success_version.clone(),
				status.is_up,
				status.server_rank,
			) {
				Some(version)
			} else {
				None
			}
		})
		.collect::<BTreeSet<_>>();
	let bracket = LiveVersionsBracket {
		min: versions.first().cloned().unwrap_or_default(),
		max: versions.last().cloned().unwrap_or_default(),
	};
	let releases = versions
		.iter()
		.map(|v| (v.0.major, v.0.minor))
		.collect::<BTreeSet<_>>();

	let mut context = Context::new();
	context.insert("title", "Server statuses");
	context.insert("entries", &entries);
	context.insert("bracket", &bracket);
	context.insert("versions", &versions);
	context.insert("releases", &releases);
	let html = tera.render("statuses", &context)?;
	Ok(Html(html))
}

async fn reload(State(AppState { db, .. }): State<AppState>) -> Result<()> {
	let mut db = db.get().await?;
	Status::ping_servers_and_save(&mut db).await?;
	Ok(())
}

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/status", get(view))
		.route("/reload", post(reload))
}
