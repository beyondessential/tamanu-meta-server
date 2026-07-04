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
			// Synthetic kind tag is always present; ungrouped, so no group tags.
			assert_eq!(tags.get("canopy:kind"), Some(&"central".to_string()));
			assert_eq!(tags.get("canopy:group-id"), None);
			assert_eq!(tags.get("canopy:group-name"), None);
			// No rank set on this server, so no synthetic rank tag.
			assert_eq!(tags.get("canopy:rank"), None);
			assert_eq!(tags.len(), 2);
		},
	)
	.await
}

/// The endpoint injects synthetic `canopy:`-prefixed tags describing the
/// server's kind, rank, and group on top of the stored tags.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_includes_synthetic_server_attributes() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group_id = Uuid::new_v4();
			let server_id = Uuid::new_v4();
			sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'synthetic-cluster')")
				.bind::<sql_types::Uuid, _>(group_id)
				.execute(&mut conn)
				.await
				.unwrap();
			sql_query(
				"INSERT INTO servers (id, host, kind, rank, device_id, group_id) \
				 VALUES ($1, 'https://s.example.com', 'facility', 'production', $2, $3)",
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
			assert_eq!(tags.get("canopy:kind"), Some(&"facility".to_string()));
			assert_eq!(tags.get("canopy:rank"), Some(&"production".to_string()));
			assert_eq!(tags.get("canopy:group-id"), Some(&group_id.to_string()));
			assert_eq!(
				tags.get("canopy:group-name"),
				Some(&"synthetic-cluster".to_string())
			);
		},
	)
	.await
}

/// A grouped server's tags include the group's effective `billing.*` labels:
/// computed defaults where the group sets nothing, and the group's explicit
/// `billing.*` tags honoured verbatim.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_includes_effective_billing_labels() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let group_id = Uuid::new_v4();
			let server_id = Uuid::new_v4();
			// Group sets an explicit billing.deployment override but leaves
			// product/stage to be computed.
			sql_query(
				"INSERT INTO server_groups (id, name, tags) \
				 VALUES ($1, 'Billing Cluster', '{\"billing.deployment\": \"acme\"}'::jsonb)",
			)
			.bind::<sql_types::Uuid, _>(group_id)
			.execute(&mut conn)
			.await
			.unwrap();
			sql_query(
				"INSERT INTO servers (id, host, kind, rank, device_id, group_id) \
				 VALUES ($1, 'https://b.example.com', 'central', 'production', $2, $3)",
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
			// Computed default product.
			assert_eq!(tags.get("billing.product"), Some(&"tamanu".to_string()));
			// Explicit group override honoured verbatim.
			assert_eq!(tags.get("billing.deployment"), Some(&"acme".to_string()));
			// Stage mapped from the highest-ranked member (production -> prod).
			assert_eq!(tags.get("billing.stage"), Some(&"prod".to_string()));
		},
	)
	.await
}

/// An ungrouped server gets no billing labels — they're a group concept.
#[tokio::test(flavor = "multi_thread")]
async fn tags_endpoint_no_billing_labels_when_ungrouped() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				"INSERT INTO servers (id, host, kind, device_id) \
				 VALUES ($1, 'https://nb.example.com', 'central', $2)",
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
			assert_eq!(tags.get("billing.product"), None);
			assert_eq!(tags.get("billing.deployment"), None);
			assert_eq!(tags.get("billing.stage"), None);
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
