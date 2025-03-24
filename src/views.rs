diesel::table! {
	latest_statuses (server_id) {
		server_id -> Uuid,
		server_created_at -> Timestamptz,
		server_updated_at -> Timestamptz,
		server_name -> Text,
		server_rank -> Text,
		server_host -> Text,

		is_up -> Bool,
		latest_latency -> Nullable<Int4>,

		latest_success_id -> Nullable<Uuid>,
		latest_success_ts -> Nullable<Timestamptz>,
		latest_success_ago -> Nullable<Interval>,
		latest_success_version -> Nullable<Text>,

		latest_error_id -> Nullable<Uuid>,
		latest_error_ts -> Nullable<Timestamptz>,
		latest_error_ago -> Nullable<Interval>,
		latest_error_message -> Nullable<Text>,
	}
}

diesel::table! {
	ordered_servers (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Text,
		rank -> Text,
		host -> Text,
		device_id -> Uuid,
	}
}

diesel::table! {
	version_updates (id) {
		id -> Uuid,
		major -> Int4,
		minor -> Int4,
		patch -> Int4,
		published -> Bool,
		changelog -> Text,
	}
}
