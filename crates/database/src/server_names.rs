//! Names a server should be reachable at, and the addresses Canopy publishes
//! for them (CRT).
//!
//! A row holds two things: the addresses the server reported, and the addresses
//! Canopy has actually published. Keeping them apart is what lets the reconcile
//! tell whether the zone already matches the intent, and what lets Canopy
//! confine itself to records it put there — in a shared zone, a record Canopy
//! did not publish is none of its business.
// spec: CRT#addresses

use std::net::IpAddr;

use commons_errors::{AppError, Result};
use commons_types::dns::normalize_domain;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use ipnet::IpNet;
use jiff::Timestamp;
use serde::Serialize;
use uuid::Uuid;

/// A name one server has registered, with what Canopy has published for it.
#[derive(Debug, Clone, Serialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::server_names)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ServerName {
	pub id: Uuid,
	/// The server that registered the name.
	pub server_id: Uuid,
	/// The name, normalised: lower case, no trailing dot.
	pub name: String,
	/// The addresses the server reported it is reachable at. Empty means the
	/// server has withdrawn the name; Canopy removes the records it published
	/// and then forgets the registration, freeing the name.
	#[schema(value_type = Vec<String>)]
	pub addresses: Vec<Option<IpNet>>,
	/// The addresses Canopy has actually published. Differs from `addresses`
	/// while a change is waiting to be reconciled.
	#[schema(value_type = Vec<String>)]
	pub published_addresses: Vec<Option<IpNet>>,
	/// When Canopy last published this name's records.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub published_at: Option<Timestamp>,
	/// Why the last publish attempt failed, if it did. Cleared on success.
	pub last_error: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
}

impl ServerName {
	/// The addresses the server asked for, as plain addresses.
	pub fn wanted(&self) -> Vec<IpAddr> {
		to_addrs(&self.addresses)
	}

	/// The addresses Canopy has published, as plain addresses.
	pub fn published(&self) -> Vec<IpAddr> {
		to_addrs(&self.published_addresses)
	}

	/// Whether the published state has caught up with what was asked for.
	pub fn is_reconciled(&self) -> bool {
		let mut wanted = self.wanted();
		let mut published = self.published();
		wanted.sort_unstable();
		published.sort_unstable();
		wanted == published
	}

	/// Whether this registration is being withdrawn: nothing wanted, and once
	/// nothing is published either the row goes.
	pub fn is_withdrawing(&self) -> bool {
		self.addresses.is_empty()
	}

	/// Register `name` for a server with the addresses it is reachable at,
	/// replacing whatever addresses were registered before.
	///
	/// An empty address list withdraws the name. `409` when the name is
	/// registered to a different server: a name's addresses are one server's to
	/// set at a time.
	pub async fn register(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		name: &str,
		addresses: &[IpAddr],
	) -> Result<Self> {
		use crate::schema::server_names::dsl;

		let name = normalize_domain(name)?;
		let nets: Vec<Option<IpNet>> = addresses.iter().map(|a| Some(to_net(*a))).collect();

		if let Some(existing) = Self::for_name(db, &name).await? {
			if existing.server_id != server_id {
				return Err(AppError::Conflict(format!(
					"{name} is registered by another server in this group; a name's addresses are \
					 one server's to set at a time"
				)));
			}
			return diesel::update(dsl::server_names.filter(dsl::id.eq(existing.id)))
				.set((
					dsl::addresses.eq(nets),
					// A fresh intent clears the previous failure: whether it
					// still applies is for the next attempt to say.
					dsl::last_error.eq::<Option<String>>(None),
				))
				.returning(Self::as_select())
				.get_result(db)
				.await
				.map_err(AppError::from);
		}

		match diesel::insert_into(dsl::server_names)
			.values((
				dsl::server_id.eq(server_id),
				dsl::name.eq(&name),
				dsl::addresses.eq(nets),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
		{
			Ok(row) => Ok(row),
			// Another server registered the same name between the read and the
			// insert.
			Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
				Err(AppError::Conflict(format!(
					"{name} is registered by another server in this group; a name's addresses are \
					 one server's to set at a time"
				)))
			}
			Err(e) => Err(AppError::from(e)),
		}
	}

	pub async fn for_name(db: &mut AsyncPgConnection, name: &str) -> Result<Option<Self>> {
		use crate::schema::server_names::dsl;
		let name = normalize_domain(name)?;
		dsl::server_names
			.select(Self::as_select())
			.filter(dsl::name.eq(name))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// The names a server has registered, by name.
	pub async fn for_server(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::server_names::dsl;
		dsl::server_names
			.select(Self::as_select())
			.filter(dsl::server_id.eq(server_id))
			.order(dsl::name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Registrations whose published state doesn't match what was asked for —
	/// the reconcile's work list, oldest change first so nothing starves.
	///
	/// Skips paused servers: while a server is paused Canopy changes no record of
	/// its, though everything already published stays published.
	// spec: CRT#pausing-a-server
	pub async fn needing_publish(db: &mut AsyncPgConnection, limit: i64) -> Result<Vec<Self>> {
		use crate::schema::{server_names, servers};
		let rows: Vec<Self> = server_names::table
			.inner_join(servers::table)
			.filter(servers::deleted_at.is_null())
			.filter(servers::name_management_paused_at.is_null())
			.select(Self::as_select())
			.order(server_names::updated_at.asc())
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)?;
		// The comparison is order-insensitive, which SQL's array inequality is
		// not, so the final say is here.
		Ok(rows
			.into_iter()
			.filter(|row| !row.is_reconciled())
			.collect())
	}

	/// Record that the published records now match `addresses`.
	pub async fn record_published(
		db: &mut AsyncPgConnection,
		id: Uuid,
		addresses: &[IpAddr],
	) -> Result<()> {
		use crate::schema::server_names::dsl;
		let nets: Vec<Option<IpNet>> = addresses.iter().map(|a| Some(to_net(*a))).collect();
		diesel::update(dsl::server_names.filter(dsl::id.eq(id)))
			.set((
				dsl::published_addresses.eq(nets),
				dsl::published_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
				dsl::last_error.eq::<Option<String>>(None),
			))
			.execute(db)
			.await?;
		Ok(())
	}

	/// Record why publishing failed, leaving the intent in place to retry.
	pub async fn record_publish_error(
		db: &mut AsyncPgConnection,
		id: Uuid,
		error: &str,
	) -> Result<()> {
		use crate::schema::server_names::dsl;
		diesel::update(dsl::server_names.filter(dsl::id.eq(id)))
			.set(dsl::last_error.eq(Some(error)))
			.execute(db)
			.await?;
		Ok(())
	}

	/// Forget a withdrawn registration, once its records are gone from the zone.
	/// This is what frees the name for another server.
	pub async fn forget(db: &mut AsyncPgConnection, id: Uuid) -> Result<()> {
		use crate::schema::server_names::dsl;
		diesel::delete(dsl::server_names.filter(dsl::id.eq(id)))
			.execute(db)
			.await?;
		Ok(())
	}
}

/// A bare address as the single-host network Postgres stores it as.
fn to_net(addr: IpAddr) -> IpNet {
	let prefix = if addr.is_ipv4() { 32 } else { 128 };
	IpNet::new(addr, prefix).expect("a host prefix is always valid")
}

fn to_addrs(nets: &[Option<IpNet>]) -> Vec<IpAddr> {
	nets.iter().flatten().map(|net| net.addr()).collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_host_address_round_trips() {
		let v4: IpAddr = "192.0.2.1".parse().unwrap();
		let v6: IpAddr = "2001:db8::1".parse().unwrap();
		assert_eq!(
			to_addrs(&[Some(to_net(v4)), Some(to_net(v6))]),
			vec![v4, v6]
		);
	}
}
