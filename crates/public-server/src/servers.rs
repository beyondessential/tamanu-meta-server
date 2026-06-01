use axum::{Json, extract::State};
use commons_errors::{ProblemDetailsSchema, Result};
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
use utoipa::ToSchema;
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(list, create, edit, remove))
}

#[derive(Debug, Serialize, ToSchema)]
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

#[utoipa::path(
	get,
	path = "/",
	tag = "servers",
	responses(
		(status = 200, description = "Publicly-listed central servers, ordered by rank then name.", body = Vec<PublicServer>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn list(State(db): State<Db>) -> Result<Json<Vec<PublicServer>>> {
	let mut db = db.get().await?;
	let mut servers = Server::list_by_kind(&mut db, ServerKind::Central, 0, None)
		.await?
		.into_iter()
		.filter_map(|s| {
			s.public_name.map(|name| PublicServer {
				name,
				host: s.host,
				rank: s.rank,
			})
		})
		.collect::<Vec<_>>();

	servers.sort_by(|a, b| {
		rank_order(&a.rank)
			.cmp(&rank_order(&b.rank))
			.then_with(|| a.name.cmp(&b.name))
	});

	Ok(Json(servers))
}

#[utoipa::path(
	post,
	path = "/",
	tag = "servers",
	security(("server-device" = [])),
	request_body = NewServer,
	responses(
		(status = 200, body = Server),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
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

#[utoipa::path(
	patch,
	path = "/",
	tag = "servers",
	security(("server-device" = [])),
	request_body = PartialServer,
	responses(
		(status = 200, body = Server),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
	),
)]
pub async fn edit(
	device: ServerDevice,
	State(db): State<Db>,
	Json(input): Json<PartialServer>,
) -> Result<Json<Server>> {
	use database::schema::servers::dsl::*;

	let mut db = db.get().await?;
	let input_id = input.id;
	let dev_id = device.0.0.id;

	// Scope to the calling device's own server: a device may only edit the
	// server it is bound to, never an arbitrary server id from the body.
	diesel::update(servers)
		.filter(id.eq(input_id))
		.filter(device_id.eq(dev_id))
		.set(input)
		.execute(&mut db)
		.await?;

	Ok(Json(
		servers
			.filter(id.eq(input_id))
			.filter(device_id.eq(dev_id))
			.select(Server::as_select())
			.first(&mut db)
			.await?,
	))
}

#[utoipa::path(
	delete,
	path = "/",
	tag = "servers",
	security(("admin-device" = [])),
	request_body = PartialServer,
	responses(
		(status = 200),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
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
