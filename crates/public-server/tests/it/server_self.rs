use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
struct SelfResponse {
	server_id: Uuid,
	device_id: Uuid,
}

/// A registered, attached device recovers its own identity: the server it is
/// enrolled as and its own device ID.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_returns_server_and_device_ids() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let server_id = Uuid::new_v4();
			sql_query(
				"WITH m AS (INSERT INTO machines (id) VALUES ($1) RETURNING id) INSERT INTO applications (id, host, type, device_id, machine_id) \
				 VALUES ($1, 'https://self.example.com', 'tamanu-central', $2, $1)",
			)
			.bind::<sql_types::Uuid, _>(server_id)
			.bind::<sql_types::Uuid, _>(device_id)
			.execute(&mut conn)
			.await
			.unwrap();

			let response = public
				.get("/servers/self")
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.await;
			response.assert_status_ok();
			let body: SelfResponse = response.json();
			assert_eq!(body.server_id, server_id);
			assert_eq!(body.device_id, device_id);
		},
	)
	.await
}

/// A device that authenticates correctly but isn't attached to any server
/// gets a 412 (precondition failed), matching the `/tags` endpoint.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_412_when_device_has_no_server() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut _conn, cert, _device_id, public, _| {
			let response = public
				.get("/servers/self")
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.await;
			response.assert_status(StatusCode::PRECONDITION_FAILED);
		},
	)
	.await
}

/// A request with no client certificate is unauthenticated.
#[tokio::test(flavor = "multi_thread")]
async fn self_endpoint_401_without_certificate() {
	commons_tests::server::run(async |_conn, public, _private| {
		let response = public.get("/servers/self").await;
		response.assert_status(StatusCode::UNAUTHORIZED);
	})
	.await
}

#[derive(Deserialize)]
struct MachineSelfResponse {
	device_id: Uuid,
	machine_id: Uuid,
	applications: Vec<Uuid>,
}

/// A box asks which box it is, and is told what runs on it. Two workloads on
/// the one machine is the case `/servers/self` cannot answer — it asks which
/// *application* the caller is, and there are two.
///
/// spec: DID#query
#[tokio::test(flavor = "multi_thread")]
async fn machine_self_answers_for_a_box_running_two_applications() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = Uuid::new_v4();
			sql_query("INSERT INTO machines (id, device_id) VALUES ($1, $2)")
				.bind::<sql_types::Uuid, _>(machine_id)
				.bind::<sql_types::Uuid, _>(device_id)
				.execute(&mut conn)
				.await
				.unwrap();
			for host in ["https://central.example", "https://facility.example"] {
				sql_query("INSERT INTO applications (host, type, machine_id) VALUES ($1, $2, $3)")
					.bind::<sql_types::Text, _>(host)
					.bind::<sql_types::Text, _>("tamanu-central")
					.bind::<sql_types::Uuid, _>(machine_id)
					.execute(&mut conn)
					.await
					.unwrap();
			}

			let response = public
				.get("/machines/self")
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.await;
			response.assert_status_ok();
			let body: MachineSelfResponse = response.json();
			assert_eq!(body.machine_id, machine_id);
			assert_eq!(body.device_id, device_id);
			assert_eq!(
				body.applications.len(),
				2,
				"both workloads on the box are named"
			);
		},
	)
	.await
}

/// A box that has enrolled but not yet reported what runs on it is awaiting a
/// report, not an error.
///
/// spec: DID#query
#[tokio::test(flavor = "multi_thread")]
async fn machine_self_answers_before_anything_has_reported() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = Uuid::new_v4();
			sql_query("INSERT INTO machines (id, device_id) VALUES ($1, $2)")
				.bind::<sql_types::Uuid, _>(machine_id)
				.bind::<sql_types::Uuid, _>(device_id)
				.execute(&mut conn)
				.await
				.unwrap();

			let response = public
				.get("/machines/self")
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.await;
			response.assert_status_ok();
			let body: MachineSelfResponse = response.json();
			assert_eq!(body.machine_id, machine_id);
			assert!(body.applications.is_empty());
		},
	)
	.await
}

/// A device row written at `server`, the name this role had before the split,
/// authenticates and reads as the machine role. Every device in the fleet was
/// written that way, so the read alias is what stops the rename locking them
/// all out — the migration rewrites the rows, and this is the belt on it.
///
/// spec: DTR
#[tokio::test(flavor = "multi_thread")]
async fn a_row_stored_as_server_reads_as_the_machine_role() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			// The harness writes the role string straight in, so the row says
			// `server` exactly as a pre-rename one does.
			#[derive(diesel::QueryableByName)]
			struct Role {
				#[diesel(sql_type = sql_types::Text)]
				role: String,
			}
			let row: Role = sql_query("SELECT role FROM devices WHERE id = $1")
				.bind::<sql_types::Uuid, _>(device_id)
				.get_result(&mut conn)
				.await
				.unwrap();
			assert_eq!(row.role, "server", "the row is written the old way");

			// The model reads it as the machine role: the column deserialises
			// through `DeviceRole`, which accepts the older spelling.
			#[derive(diesel::QueryableByName)]
			struct StoredRole {
				#[diesel(sql_type = sql_types::Text)]
				role: commons_types::device::DeviceRole,
			}
			let parsed: StoredRole = sql_query("SELECT role FROM devices WHERE id = $1")
				.bind::<sql_types::Uuid, _>(device_id)
				.get_result(&mut conn)
				.await
				.unwrap();
			assert_eq!(parsed.role, commons_types::device::DeviceRole::Machine);

			// And it authenticates: a fielded agent keeps working.
			let machine_id = Uuid::new_v4();
			sql_query("INSERT INTO machines (id, device_id) VALUES ($1, $2)")
				.bind::<sql_types::Uuid, _>(machine_id)
				.bind::<sql_types::Uuid, _>(device_id)
				.execute(&mut conn)
				.await
				.unwrap();
			public
				.get("/machines/self")
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.await
				.assert_status_ok();
		},
	)
	.await
}
