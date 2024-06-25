// @generated automatically by Diesel CLI.

diesel::table! {
	servers (id) {
		id -> Uuid,
		created_at -> Timestamp,
		updated_at -> Timestamp,
		name -> Text,
		rank -> Text,
		host -> Text,
	}
}
