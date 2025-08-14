use axum::{
	Json,
	extract::State,
	routing::{Router, delete, get, patch, post},
};
use diesel::{ExpressionMethods as _, QueryDsl as _, SelectableHelper as _};
use diesel_async::RunQueryDsl as _;
use serde::Serialize;

use crate::{
	db::{
		server_kind::ServerKind,
		server_rank::ServerRank,
		servers::{NewServer, PartialServer, Server},
		url_field::UrlField,
	},
	error::Result,
	servers::device_auth::{AdminDevice, ServerDevice},
	state::{AppState, Db},
};

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

pub async fn list(State(db): State<Db>) -> Result<Json<Vec<PublicServer>>> {
	let mut db = db.get().await?;
	Ok(Json(
		Server::get_all(&mut db)
			.await?
			.into_iter()
			.filter_map(|s| {
				(s.kind == ServerKind::Central)
					.then(|| {
						s.name.map(|name| PublicServer {
							name,
							host: s.host,
							rank: s.rank,
						})
					})
					.flatten()
			})
			.collect(),
	))
}

pub async fn create(
	device: ServerDevice,
	State(db): State<Db>,
	Json(input): Json<NewServer>,
) -> Result<Json<Server>> {
	let mut db = db.get().await?;
	let mut input = Server::from(input);
	input.device_id = Some(device.0.id);

	let server = diesel::insert_into(crate::views::ordered_servers::table)
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
	use crate::views::ordered_servers::dsl::*;

	let mut db = db.get().await?;
	let input_id = input.id;

	diesel::update(ordered_servers)
		.filter(id.eq(input_id))
		.set(input)
		.execute(&mut db)
		.await?;

	Ok(Json(
		ordered_servers
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
	use crate::schema::servers::dsl::*;

	let mut db = db.get().await?;

	diesel::delete(servers)
		.filter(id.eq(input.id))
		.execute(&mut db)
		.await?;

	Ok(())
}
