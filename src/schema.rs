// @generated automatically by Diesel CLI.

pub mod sql_types {
	#[derive(diesel::query_builder::QueryId, diesel::sql_types::SqlType)]
	#[diesel(postgres_type(name = "device_role"))]
	pub struct DeviceRole;
}

diesel::table! {
	device_connections (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		device -> Bytea,
		ip -> Inet,
		tls_version -> Nullable<Text>,
		latency -> Nullable<Interval>,
		user_agent -> Nullable<Text>,
		tamanu_version -> Nullable<Text>,
		status -> Jsonb,
	}
}

diesel::table! {
	device_trust (device, trusts) {
		device -> Bytea,
		trusts -> Bytea,
		created_at -> Timestamptz,
	}
}

diesel::table! {
	use diesel::sql_types::*;
	use super::sql_types::DeviceRole;

	devices (public_key) {
		public_key -> Bytea,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		role -> DeviceRole,
	}
}

diesel::table! {
	servers (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Text,
		rank -> Text,
		host -> Text,
		owner -> Nullable<Bytea>,
	}
}

diesel::table! {
	statuses (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		server_id -> Uuid,
		latency_ms -> Nullable<Int4>,
		version -> Nullable<Text>,
		error -> Nullable<Text>,
	}
}

diesel::joinable!(device_connections -> devices (device));
diesel::joinable!(servers -> devices (owner));
diesel::joinable!(statuses -> servers (server_id));

diesel::allow_tables_to_appear_in_same_query!(
	device_connections,
	device_trust,
	devices,
	servers,
	statuses,
);
