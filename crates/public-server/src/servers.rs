use axum::{
	Json,
	extract::State,
	routing::{Router, delete, get, patch, post},
};
use commons_errors::Result;
use commons_servers::device_auth::{AdminDevice, ServerDevice};
use commons_types::server::{kind::ServerKind, rank::ServerRank};
use database::{
	Db,
	servers::{NewServer, PartialServer, Server},
	url_field::UrlField,
};
use diesel::{ExpressionMethods as _, QueryDsl as _, SelectableHelper as _};
use diesel_async::RunQueryDsl as _;
use serde::Serialize;

use crate::state::AppState;

pub fn routes() -> Router<AppState> {
	Router::new()
		.route("/", get(list))
		.route("/", post(create))
		.route("/", patch(edit))
		.route("/", delete(remove))
}

#[derive(Debug, Serialize)]
pub struct PublicServer {
	pub name: String,
	pub host: UrlField,
	pub rank: Option<ServerRank>,
}

fn rank_order(rank: &Option<ServerRank>) -> u32 {
	match rank {
		Some(ServerRank::Production) => 0,
		Some(ServerRank::Clone) => 1,
		Some(ServerRank::Demo) => 2,
		Some(ServerRank::Test) => 3,
		Some(ServerRank::Dev) => 4,
		_ => 5,
	}
}

pub async fn list(State(db): State<Db>) -> Result<Json<Vec<PublicServer>>> {
	let mut db = db.get().await?;
	let mut servers = Server::get_all(&mut db)
		.await?
		.into_iter()
		.filter_map(|s| {
			(s.kind == ServerKind::Central && s.listed)
				.then(|| s.name.map(|name| (name, s.host, s.rank)))
				.flatten()
		})
		.map(|(name, host, rank)| {
			(
				PublicServer {
					name: name.clone(),
					host,
					rank,
				},
				name,
			)
		})
		.collect::<Vec<_>>();

	servers.sort_by(|(a, a_name), (b, b_name)| {
		rank_order(&a.rank)
			.cmp(&rank_order(&b.rank))
			.then_with(|| a_name.cmp(b_name))
	});

	Ok(Json(servers.into_iter().map(|(s, _)| s).collect()))
}

pub async fn create(
	device: ServerDevice,
	State(db): State<Db>,
	Json(input): Json<NewServer>,
) -> Result<Json<Server>> {
	let mut db = db.get().await?;
	let mut input = Server::from(input);
	input.device_id = Some(device.0.0.id);

	let server = diesel::insert_into(database::schema::servers::table)
		.values(input)
		.returning(Server::as_select())
		.get_result(&mut db)
		.await?;

	Ok(Json(server))
}

pub async fn edit(
	_device: ServerDevice,
	State(db): State<Db>,
	Json(input): Json<PartialServer>,
) -> Result<Json<Server>> {
	use database::schema::servers::dsl::*;

	let mut db = db.get().await?;
	let input_id = input.id;

	diesel::update(servers)
		.filter(id.eq(input_id))
		.set(input)
		.execute(&mut db)
		.await?;

	Ok(Json(
		servers
			.filter(id.eq(input_id))
			.select(Server::as_select())
			.first(&mut db)
			.await?,
	))
}

pub async fn remove(
	_device: AdminDevice,
	State(db): State<Db>,
	Json(input): Json<PartialServer>,
) -> Result<()> {
	use database::schema::servers::dsl::*;

	let mut db = db.get().await?;

	diesel::delete(servers)
		.filter(id.eq(input.id))
		.execute(&mut db)
		.await?;

	Ok(())
}
