use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use http::StatusCode;
use std::collections::HashMap;
use uuid::Uuid;

/// Group-only tags propagate through the merged endpoint when the server
/// has no tags of its own.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_returns_group_tags_when_server_has_none() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group_id = Uuid::new_v4();
			let server_id = Uuid::new_v4();
			sql_query(
				"INSERT INTO server_groups (id, name, tags) \
				 VALUES ($1, 'tagged-cluster', '{\"region\": \"au\", \"tier\": \"1\"}'::jsonb)",
			)
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await
			.unwrap();
			sql_query(
				"INSERT INTO servers (id, host, kind, device_id, group_id) \
				 VALUES ($1, 'https://t.example.com', 'central', $2, $3)",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Uuid, _>(device_id)
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await
			.unwrap();

			let response = public
				.get("/tags")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status_ok();
			let tags: HashMap<String, String> = response.json();
			assert_eq!(tags.get("region"), Some(&"au".to_string()));
			assert_eq!(tags.get("tier"), Some(&"1".to_string()));
		},
	)
	.await
}

/// Server tags win on key collision; non-colliding group keys carry through.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_overlays_server_tags_onto_group() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group_id = Uuid::new_v4();
			let server_id = Uuid::new_v4();
			sql_query(
				"INSERT INTO server_groups (id, name, tags) \
				 VALUES ($1, 'overlay-cluster', '{\"env\": \"group\", \"tier\": \"1\"}'::jsonb)",
			)
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await
			.unwrap();
			sql_query(
				"INSERT INTO servers (id, host, kind, device_id, group_id, tags) \
				 VALUES ($1, 'https://o.example.com', 'central', $2, $3, '{\"env\": \"server\", \"region\": \"au\"}'::jsonb)",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Uuid, _>(device_id)
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await
			.unwrap();

			let response = public
				.get("/tags")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status_ok();
			let tags: HashMap<String, String> = response.json();
			// Server overrides on the colliding key.
			assert_eq!(tags.get("env"), Some(&"server".to_string()));
			// Group's non-colliding key carries through.
			assert_eq!(tags.get("tier"), Some(&"1".to_string()));
			// Server's exclusive key is present.
			assert_eq!(tags.get("region"), Some(&"au".to_string()));
		},
	)
	.await
}

/// An ungrouped server returns just its own tags — no group overlay.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_returns_server_tags_when_ungrouped() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				"INSERT INTO servers (id, host, kind, device_id, tags) \
				 VALUES ($1, 'https://lone.example.com', 'central', $2, '{\"role\": \"primary\"}'::jsonb)",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Uuid, _>(device_id)
			.execute(&mut conn)
			.await
			.unwrap();

			let response = public
				.get("/tags")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status_ok();
			let tags: HashMap<String, String> = response.json();
			assert_eq!(tags.get("role"), Some(&"primary".to_string()));
			assert_eq!(tags.len(), 1);
		},
	)
	.await
}

/// A device that authenticates correctly but isn't attached to any server
/// gets a 412 (precondition failed) — same code the events endpoint uses
/// for the same situation.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_412_when_device_has_no_server() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut _conn, cert, _device_id, public, _| {
			let response = public
				.get("/tags")
				.add_header("mtls-certificate", &cert)
				.await;
			response.assert_status(StatusCode::PRECONDITION_FAILED);
		},
	)
	.await
}
