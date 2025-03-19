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
		device_id -> Uuid,
		ip -> Inet,
		user_agent -> Nullable<Text>,
	}
}

diesel::table! {
	use diesel::sql_types::*;
	use super::sql_types::DeviceRole;

	devices (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		key_data -> Bytea,
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
		device_id -> Nullable<Uuid>,
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

diesel::joinable!(device_connections -> devices (device_id));
diesel::joinable!(servers -> devices (device_id));
diesel::joinable!(statuses -> servers (server_id));

diesel::allow_tables_to_appear_in_same_query!(device_connections, devices, servers, statuses,);
