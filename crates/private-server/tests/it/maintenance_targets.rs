//! A maintenance window is declared over a machine, so what reads as suspended
//! is the applications on that machine.
//!
//! Every machine that predates the split took its application's id, so a reader
//! comparing a window's machine against an application's own id agreed with a
//! correct one on all existing data. It parts company the moment a machine is
//! created with an id of its own, which is what these seed.
//!
//! spec: MNT

use commons_tests::diesel_async::SimpleAsyncConnection;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct GroupDetail {
	applications: Vec<ApplicationRow>,
}

#[derive(Debug, Deserialize)]
struct ApplicationRow {
	id: String,
	maintained: Option<bool>,
}

/// A machine and one application on it, with deliberately unequal ids.
async fn seed(conn: &mut impl SimpleAsyncConnection, group: Uuid, name: &str) -> (Uuid, Uuid) {
	let machine = Uuid::new_v4();
	let application = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO machines (id, group_id) VALUES ('{machine}', '{group}'); \
		 INSERT INTO applications (id, name, host, type, group_id, machine_id) \
		 VALUES ('{application}', '{name}', 'https://{application}.example.com', \
		         'tamanu-central', '{group}', '{machine}')"
	))
	.await
	.expect("seed machine and application");
	(machine, application)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_window_over_a_machine_suspends_the_applications_on_it() {
	commons_tests::server::run(async |mut conn, _, private| {
		let group = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group}', 'mnt-group')"
		))
		.await
		.unwrap();

		let (under, under_app) = seed(&mut conn, group, "under-maintenance").await;
		let (_, beside_app) = seed(&mut conn, group, "beside-it").await;

		conn.batch_execute(&format!(
			"INSERT INTO maintenance_windows (machine_id, expected_end) \
			 VALUES ('{under}', NOW() + INTERVAL '1 hour')"
		))
		.await
		.unwrap();

		let response = private
			.post("/api/server_groups/get")
			.json(&serde_json::json!({ "server_group_id": group }))
			.await;
		response.assert_status_ok();
		let detail: GroupDetail = response.json();

		let find = |id: Uuid| {
			detail
				.applications
				.iter()
				.find(|a| a.id == id.to_string())
				.unwrap_or_else(|| panic!("{id} missing from the group"))
		};

		assert_eq!(
			find(under_app).maintained,
			Some(true),
			"the workload on the box under maintenance is suspended"
		);
		assert_eq!(
			find(beside_app).maintained,
			Some(false),
			"a workload on another box is not"
		);
	})
	.await
}

/// A group-wide window suspends every application in the group, whichever box
/// each runs on.
#[tokio::test(flavor = "multi_thread")]
async fn a_window_over_a_group_suspends_all_of_it() {
	commons_tests::server::run(async |mut conn, _, private| {
		let group = Uuid::new_v4();
		conn.batch_execute(&format!(
			"INSERT INTO server_groups (id, name) VALUES ('{group}', 'whole-group')"
		))
		.await
		.unwrap();

		let (_, first) = seed(&mut conn, group, "first").await;
		let (_, second) = seed(&mut conn, group, "second").await;

		conn.batch_execute(&format!(
			"INSERT INTO maintenance_windows (server_group_id, expected_end) \
			 VALUES ('{group}', NOW() + INTERVAL '1 hour')"
		))
		.await
		.unwrap();

		let response = private
			.post("/api/server_groups/get")
			.json(&serde_json::json!({ "server_group_id": group }))
			.await;
		response.assert_status_ok();
		let detail: GroupDetail = response.json();

		for id in [first, second] {
			let row = detail
				.applications
				.iter()
				.find(|a| a.id == id.to_string())
				.unwrap_or_else(|| panic!("{id} missing from the group"));
			assert_eq!(row.maintained, Some(true));
		}
	})
	.await
}
