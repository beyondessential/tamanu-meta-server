// @generated automatically by Diesel CLI.

diesel::table! {
	servers (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Text,
		rank -> Text,
		host -> Text,
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

diesel::joinable!(statuses -> servers (server_id));

diesel::allow_tables_to_appear_in_same_query!(servers, statuses,);
