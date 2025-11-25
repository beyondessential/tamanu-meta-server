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
	}
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
	}
}

diesel::table! {
	statuses (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		server_id -> Uuid,
		version -> Nullable<Text>,
		extra -> Jsonb,
		device_id -> Nullable<Uuid>,
	}
}

diesel::table! {
	test_metrics (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_changed_except (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_changed_only (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_changed_simple (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_combo (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_inverted (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_multi (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
	}
}

diesel::table! {
	test_metrics_normal (id) {
		id -> Int4,
		name -> Text,
		value -> Float4,
		error_count -> Int4,
		created_at -> Nullable<Timestamp>,
		updated_at -> Nullable<Timestamp>,
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
	}
}

diesel::joinable!(artifacts -> versions (version_id));
diesel::joinable!(device_connections -> devices (device_id));
diesel::joinable!(device_keys -> devices (device_id));
diesel::joinable!(servers -> devices (device_id));
diesel::joinable!(statuses -> devices (device_id));
diesel::joinable!(statuses -> servers (server_id));

diesel::allow_tables_to_appear_in_same_query!(
	admins,
	artifacts,
	device_connections,
	device_keys,
	devices,
	servers,
	statuses,
	test_metrics,
	test_metrics_changed_except,
	test_metrics_changed_only,
	test_metrics_changed_simple,
	test_metrics_combo,
	test_metrics_inverted,
	test_metrics_multi,
	test_metrics_normal,
	versions,
);
