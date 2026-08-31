//! Which secret variables an environment or a server carries.
//!
//! Only the names and who set them live here. The values are held in canopy's
//! secret store, so listing what an environment carries, and refusing a tag that
//! collides with one, never reads a value.
// spec: INV#secret-variables

use commons_errors::{AppError, Result};
use commons_types::server::rank::ServerRank;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

/// A declared secret variable: its name, the scope it is set at, and who last
/// set it.
#[derive(Clone, Debug, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::inventory_secret_variables)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct InventorySecretVariable {
	/// Unique identifier of this declaration.
	pub id: Uuid,
	/// Set with `rank` for a variable belonging to one environment.
	pub server_group_id: Option<Uuid>,
	/// The rank of the environment, set alongside `server_group_id`.
	pub rank: Option<ServerRank>,
	/// Set for a variable belonging to one server.
	pub server_id: Option<Uuid>,
	/// The variable's name, and the key its value is stored under.
	pub name: String,
	/// The login that last set the value.
	pub set_by: Option<String>,
	/// When the name was first set here.
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When its value was last replaced.
	#[diesel(deserialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

/// Where a secret variable is set: one environment, or one server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecretScope {
	Environment { group_id: Uuid, rank: ServerRank },
	Server { server_id: Uuid },
}

impl InventorySecretVariable {
	/// The names an environment carries, sorted.
	pub async fn list_for_environment(
		conn: &mut AsyncPgConnection,
		group_id: Uuid,
		rank: ServerRank,
	) -> Result<Vec<Self>> {
		use crate::schema::inventory_secret_variables::dsl;

		dsl::inventory_secret_variables
			.filter(dsl::server_group_id.eq(group_id))
			.filter(dsl::rank.eq(rank.to_string()))
			.filter(dsl::server_id.is_null())
			.order(dsl::name.asc())
			.select(Self::as_select())
			.get_results(conn)
			.await
			.map_err(AppError::from)
	}

	/// The names the given servers carry, sorted by server then name.
	pub async fn list_for_servers(
		conn: &mut AsyncPgConnection,
		server_ids: &[Uuid],
	) -> Result<Vec<Self>> {
		use crate::schema::inventory_secret_variables::dsl;

		if server_ids.is_empty() {
			return Ok(Vec::new());
		}
		dsl::inventory_secret_variables
			.filter(dsl::server_id.eq_any(server_ids))
			.order((dsl::server_id.asc(), dsl::name.asc()))
			.select(Self::as_select())
			.get_results(conn)
			.await
			.map_err(AppError::from)
	}

	/// Every declaration under a group: its environments' and those of the
	/// servers in it. Backs the collision check on a group tag write, where a
	/// name set at any rank is a collision.
	pub async fn list_under_group(
		conn: &mut AsyncPgConnection,
		group_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::{inventory_secret_variables::dsl, servers};

		let server_ids: Vec<Uuid> = servers::table
			.filter(servers::group_id.eq(group_id))
			.select(servers::id)
			.get_results(conn)
			.await
			.map_err(AppError::from)?;

		dsl::inventory_secret_variables
			.filter(
				dsl::server_group_id
					.eq(group_id)
					.or(dsl::server_id.eq_any(server_ids)),
			)
			.order(dsl::name.asc())
			.select(Self::as_select())
			.get_results(conn)
			.await
			.map_err(AppError::from)
	}

	/// Record that `name` is set at `scope`, or refresh who set it. The value
	/// itself is written to the secret store by the caller.
	pub async fn declare(
		conn: &mut AsyncPgConnection,
		scope: SecretScope,
		name: &str,
		set_by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::inventory_secret_variables::dsl;

		let existing: Option<Uuid> = match scope {
			SecretScope::Environment { group_id, rank } => dsl::inventory_secret_variables
				.filter(dsl::name.eq(name))
				.filter(dsl::server_group_id.eq(group_id))
				.filter(dsl::rank.eq(rank.to_string()))
				.filter(dsl::server_id.is_null())
				.select(dsl::id)
				.first(conn)
				.await
				.optional()
				.map_err(AppError::from)?,
			SecretScope::Server { server_id } => dsl::inventory_secret_variables
				.filter(dsl::name.eq(name))
				.filter(dsl::server_id.eq(server_id))
				.select(dsl::id)
				.first(conn)
				.await
				.optional()
				.map_err(AppError::from)?,
		};

		if let Some(id) = existing {
			return diesel::update(dsl::inventory_secret_variables.find(id))
				.set((dsl::set_by.eq(set_by), dsl::updated_at.eq(diesel::dsl::now)))
				.returning(Self::as_select())
				.get_result(conn)
				.await
				.map_err(AppError::from);
		}

		let (group_id, rank, server_id) = match scope {
			SecretScope::Environment { group_id, rank } => {
				(Some(group_id), Some(rank.to_string()), None)
			}
			SecretScope::Server { server_id } => (None, None, Some(server_id)),
		};
		diesel::insert_into(dsl::inventory_secret_variables)
			.values((
				dsl::server_group_id.eq(group_id),
				dsl::rank.eq(rank),
				dsl::server_id.eq(server_id),
				dsl::name.eq(name),
				dsl::set_by.eq(set_by),
			))
			.returning(Self::as_select())
			.get_result(conn)
			.await
			.map_err(AppError::from)
	}

	/// Forget a declaration. Answers whether there was one.
	pub async fn remove(
		conn: &mut AsyncPgConnection,
		scope: SecretScope,
		name: &str,
	) -> Result<bool> {
		use crate::schema::inventory_secret_variables::dsl;

		let deleted = match scope {
			SecretScope::Environment { group_id, rank } => diesel::delete(
				dsl::inventory_secret_variables
					.filter(dsl::name.eq(name))
					.filter(dsl::server_group_id.eq(group_id))
					.filter(dsl::rank.eq(rank.to_string()))
					.filter(dsl::server_id.is_null()),
			)
			.execute(conn)
			.await
			.map_err(AppError::from)?,
			SecretScope::Server { server_id } => diesel::delete(
				dsl::inventory_secret_variables
					.filter(dsl::name.eq(name))
					.filter(dsl::server_id.eq(server_id)),
			)
			.execute(conn)
			.await
			.map_err(AppError::from)?,
		};
		Ok(deleted > 0)
	}
}

/// A secret variable name a tag may not take, given where the tag is being set.
///
/// A server's tag collides with a secret of that server or of its environment;
/// a group's tag collides with a secret set anywhere under the group, since the
/// group's tags reach every environment in it.
pub async fn colliding_name(
	conn: &mut AsyncPgConnection,
	scope: TagScope,
	keys: impl Iterator<Item = &str>,
) -> Result<Option<String>> {
	let declared: Vec<InventorySecretVariable> = match scope {
		TagScope::Group { group_id } => {
			InventorySecretVariable::list_under_group(conn, group_id).await?
		}
		TagScope::Server {
			server_id,
			group_id,
			rank,
		} => {
			let mut declared =
				InventorySecretVariable::list_for_servers(conn, &[server_id]).await?;
			if let Some(group_id) = group_id {
				declared.extend(
					InventorySecretVariable::list_for_environment(conn, group_id, rank).await?,
				);
			}
			declared
		}
	};

	let names: std::collections::BTreeSet<&str> =
		declared.iter().map(|var| var.name.as_str()).collect();
	Ok(keys
		.filter(|key| names.contains(key))
		.map(str::to_owned)
		.next())
}

/// Where a tag is being written, for the collision check.
#[derive(Clone, Copy, Debug)]
pub enum TagScope {
	Group {
		group_id: Uuid,
	},
	Server {
		server_id: Uuid,
		group_id: Option<Uuid>,
		rank: ServerRank,
	},
}

/// Refuse a tag write that takes a name already set as a secret variable in
/// scope. A name is one or the other: which of the two a run received would
/// otherwise turn on the order the merge happened to take.
pub async fn reject_secret_names(
	conn: &mut AsyncPgConnection,
	scope: TagScope,
	tags: &commons_types::server::TagMap,
) -> Result<()> {
	match colliding_name(conn, scope, tags.0.keys().map(String::as_str)).await? {
		Some(name) => Err(AppError::BadRequest(format!(
			"{name:?} is set as a secret variable here, so it cannot also be a tag"
		))),
		None => Ok(()),
	}
}
