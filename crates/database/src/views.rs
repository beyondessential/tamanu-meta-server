diesel::table! {
	ordered_servers (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Nullable<Text>,
		kind -> Text,
		rank -> Nullable<Text>,
		host -> Text,
		device_id -> Nullable<Uuid>,
	}
}

diesel::table! {
	version_updates (id) {
		id -> Uuid,
		major -> Int4,
		minor -> Int4,
		patch -> Int4,
		status -> crate::schema::sql_types::VersionStatus,
		changelog -> Text,
	}
}
