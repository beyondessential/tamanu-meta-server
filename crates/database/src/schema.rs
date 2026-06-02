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
		version_id -> Nullable<Uuid>,
		artifact_type -> Text,
		platform -> Text,
		download_url -> Text,
		device_id -> Nullable<Uuid>,
		version_range_pattern -> Nullable<Text>,
	}
}

diesel::table! {
	bestool_snippets (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		deleted_at -> Nullable<Timestamptz>,
		supersedes_id -> Nullable<Uuid>,
		editor -> Text,
		name -> Text,
		description -> Nullable<Text>,
		sql -> Text,
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
	device_server_associations (device_id, server_id) {
		device_id -> Uuid,
		server_id -> Uuid,
		first_seen -> Timestamptz,
		last_seen -> Timestamptz,
	}
}

diesel::table! {
	devices (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		role -> Text,
		tailscale_node_id -> Nullable<Text>,
		tailscale_node_name -> Nullable<Text>,
		tailscale_tailnet -> Nullable<Text>,
	}
}

diesel::table! {
	events (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		occurred_at -> Nullable<Timestamptz>,
		issue_id -> Uuid,
		severity -> Text,
		description -> Nullable<Text>,
		message -> Text,
		active -> Bool,
		hash -> Bytea,
		occurrences -> Int4,
		last_seen -> Timestamptz,
	}
}

diesel::table! {
	healthcheck_severities (check_name) {
		check_name -> Text,
		severity -> Text,
		first_seen -> Timestamptz,
		reviewed_at -> Nullable<Timestamptz>,
		reviewed_by -> Nullable<Text>,
		notes -> Nullable<Text>,
		updated_at -> Timestamptz,
		rules -> Nullable<Jsonb>,
	}
}

diesel::table! {
	incident_issues (incident_id, issue_id, joined_at) {
		incident_id -> Uuid,
		issue_id -> Uuid,
		joined_at -> Timestamptz,
		left_at -> Nullable<Timestamptz>,
	}
}

diesel::table! {
	incident_notes (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		incident_id -> Uuid,
		author -> Text,
		body -> Text,
	}
}

diesel::table! {
	incidents (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		opened_at -> Timestamptz,
		closed_at -> Nullable<Timestamptz>,
		resolved_at -> Nullable<Timestamptz>,
		resolved_by -> Nullable<Text>,
		resolved_reason -> Nullable<Text>,
		server_group_id -> Uuid,
		escalated_at -> Nullable<Timestamptz>,
	}
}

diesel::table! {
	issue_notes (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		issue_id -> Uuid,
		author -> Text,
		body -> Text,
	}
}

diesel::table! {
	issues (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		server_id -> Uuid,
		device_id -> Nullable<Uuid>,
		source -> Text,
		#[sql_name = "ref"]
		ref_ -> Text,
		severity -> Text,
		description -> Nullable<Text>,
		message -> Text,
		active -> Bool,
		first_seen -> Timestamptz,
		last_seen -> Timestamptz,
		resolved_at -> Nullable<Timestamptz>,
		resolved_by -> Nullable<Text>,
		resolved_reason -> Nullable<Text>,
		snoozed_until -> Nullable<Timestamptz>,
	}
}

diesel::table! {
	server_group_silenced_refs (server_group_id, source, ref_) {
		server_group_id -> Uuid,
		source -> Text,
		#[sql_name = "ref"]
		ref_ -> Text,
		created_at -> Timestamptz,
		created_by -> Nullable<Text>,
	}
}

diesel::table! {
	server_groups (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
		name -> Text,
		notes -> Text,
		tags -> Jsonb,
		slack_open_delay -> Interval,
		version_server_id -> Nullable<Uuid>,
		effective_version -> Nullable<Text>,
	}
}

diesel::table! {
	server_silenced_refs (server_id, source, ref_) {
		server_id -> Uuid,
		source -> Text,
		#[sql_name = "ref"]
		ref_ -> Text,
		created_at -> Timestamptz,
		created_by -> Nullable<Text>,
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
		cloud -> Nullable<Bool>,
		geolocation -> Nullable<Array<Nullable<Float8>>>,
		alert_when_down_for -> Interval,
		group_id -> Nullable<Uuid>,
		notes -> Text,
		tags -> Jsonb,
		public_name -> Nullable<Text>,
		is_monitored -> Bool,
	}
}

diesel::table! {
	slack_outbox (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		kind -> Text,
		incident_id -> Uuid,
		issue_id -> Nullable<Uuid>,
		note_id -> Nullable<Uuid>,
		payload -> Jsonb,
		delivered_at -> Nullable<Timestamptz>,
		attempts -> Int4,
		last_error -> Nullable<Text>,
		last_response -> Nullable<Text>,
		gave_up_at -> Nullable<Timestamptz>,
		deliver_after -> Timestamptz,
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
		healthy -> Bool,
		health -> Jsonb,
	}
}

diesel::table! {
	tailscale_users (login) {
		login -> Text,
		name -> Text,
		profile_pic -> Nullable<Text>,
		created_at -> Timestamptz,
		updated_at -> Timestamptz,
	}
}

diesel::table! {
	version_known_issues (id) {
		id -> Uuid,
		created_at -> Timestamptz,
		author -> Text,
		description -> Text,
		resolved_at -> Nullable<Timestamptz>,
		resolved_by -> Nullable<Text>,
		resolution_message -> Nullable<Text>,
		min_major -> Int4,
		min_minor -> Int4,
		min_patch -> Int4,
		max_major -> Nullable<Int4>,
		max_minor -> Nullable<Int4>,
		max_patch -> Nullable<Int4>,
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
diesel::joinable!(device_server_associations -> devices (device_id));
diesel::joinable!(device_server_associations -> servers (server_id));
diesel::joinable!(events -> issues (issue_id));
diesel::joinable!(incident_issues -> incidents (incident_id));
diesel::joinable!(incident_issues -> issues (issue_id));
diesel::joinable!(incident_notes -> incidents (incident_id));
diesel::joinable!(incidents -> server_groups (server_group_id));
diesel::joinable!(issue_notes -> issues (issue_id));
diesel::joinable!(issues -> devices (device_id));
diesel::joinable!(issues -> servers (server_id));
diesel::joinable!(server_group_silenced_refs -> server_groups (server_group_id));
diesel::joinable!(server_silenced_refs -> servers (server_id));
diesel::joinable!(servers -> devices (device_id));
diesel::joinable!(slack_outbox -> incident_notes (note_id));
diesel::joinable!(slack_outbox -> incidents (incident_id));
diesel::joinable!(slack_outbox -> issues (issue_id));
diesel::joinable!(statuses -> devices (device_id));
diesel::joinable!(statuses -> servers (server_id));
diesel::joinable!(versions -> devices (device_id));

diesel::allow_tables_to_appear_in_same_query!(
	admins,
	artifacts,
	bestool_snippets,
	chrome_releases,
	device_connections,
	device_keys,
	device_server_associations,
	devices,
	events,
	healthcheck_severities,
	incident_issues,
	incident_notes,
	incidents,
	issue_notes,
	issues,
	server_group_silenced_refs,
	server_groups,
	server_silenced_refs,
	servers,
	slack_outbox,
	sql_playground_history,
	statuses,
	tailscale_users,
	version_known_issues,
	versions,
);
