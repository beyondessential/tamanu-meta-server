diesel::table! {
	version_updates (id) {
		id -> Uuid,
		major -> Int4,
		minor -> Int4,
		patch -> Int4,
		status -> Text,
		changelog -> Text,
	}
}
