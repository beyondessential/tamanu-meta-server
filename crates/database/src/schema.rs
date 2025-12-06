// @generated automatically by Diesel CLI.

diesel::table! {
	admins (email) {
		email -> Text,
		created_at -> Timestamptz,
	}
}

diesel::table! {
	artifacts (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		version_id -> Uuid,
		artifact_type -> Text,
		platform -> Text,
		download_url -> Text,
		device_id -> Nullable<Uuid>,
	}
}

diesel::table! {
	chrome_releases (version) {
		version -> Text,
		release_date -> Text,
		is_eol -> Bool,
		eol_from -> Nullable<Text>,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
	}
}

diesel::table! {
	device_connections (id, created_at) {
		id -> Uuid,
		created_at -> Timestamptz,
		device_id -> Uuid,
		ip -> Inet,
		user_agent -> Nullable<Text>,
	}
}

diesel::table! {
	device_keys (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		device_id -> Uuid,
		key_data -> Bytea,
		name -> Nullable<Text>,
		is_active -> Bool,
	}
}

diesel::table! {
	devices (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		role -> Text,
	}
}

diesel::table! {
	servers (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Nullable<Text>,
		rank -> Nullable<Text>,
		host -> Text,
		device_id -> Nullable<Uuid>,
		kind -> Text,
		parent_server_id -> Nullable<Uuid>,
		listed -> Bool,
		cloud -> Nullable<Bool>,
		geolocation -> Nullable<Array<Nullable<Float8>>>,
	}
}

diesel::table! {
	sql_playground_history (id) {
		id -> Uuid,
		query -> Text,
		tailscale_user -> Text,
		created_at -> Timestamptz,
	}
}

diesel::table! {
	statuses (id, created_at) {
		id -> Uuid,
		created_at -> Timestamptz,
		server_id -> Uuid,
		version -> Nullable<Text>,
		extra -> Jsonb,
		device_id -> Nullable<Uuid>,
	}
}

diesel::table! {
	versions (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		major -> Int4,
		minor -> Int4,
		patch -> Int4,
		changelog -> Text,
		status -> Text,
		device_id -> Nullable<Uuid>,
	}
}

diesel::joinable!(artifacts -> devices (device_id));
diesel::joinable!(artifacts -> versions (version_id));
diesel::joinable!(device_connections -> devices (device_id));
diesel::joinable!(device_keys -> devices (device_id));
diesel::joinable!(servers -> devices (device_id));
diesel::joinable!(statuses -> devices (device_id));
diesel::joinable!(statuses -> servers (server_id));
diesel::joinable!(versions -> devices (device_id));

diesel::allow_tables_to_appear_in_same_query!(
	admins,
	artifacts,
	chrome_releases,
	device_connections,
	device_keys,
	devices,
	servers,
	sql_playground_history,
	statuses,
	versions,
);
