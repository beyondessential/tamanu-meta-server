//! The split push shape: a reporter that separates the machine's material
//! from each application's, and names each application by a key of its own.
//!
//! spec: STA

use diesel::{QueryableByName, sql_query, sql_types};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
struct TargetResponse {
	tags: std::collections::BTreeMap<String, String>,
	check_severities: std::collections::BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct SplitResponse {
	machine: Option<TargetResponse>,
	applications: Option<std::collections::BTreeMap<String, TargetResponse>>,
}

#[derive(QueryableByName, Debug)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

#[derive(QueryableByName, Debug)]
struct Row {
	#[diesel(sql_type = sql_types::Jsonb)]
	extra: serde_json::Value,
	#[diesel(sql_type = sql_types::Jsonb)]
	health: serde_json::Value,
}

#[derive(QueryableByName, Debug)]
struct Active {
	#[diesel(sql_type = sql_types::Bool)]
	active: bool,
}

/// A box in a group, with the calling device enrolled on it. Applications are
/// left to the pushes, which is the point of the shape.
async fn machine_for(conn: &mut AsyncPgConnection, device_id: Uuid) -> Uuid {
	let group_id = Uuid::new_v4();
	sql_query("INSERT INTO server_groups (id, name) VALUES ($1, 'split-group')")
		.bind::<sql_types::Uuid, _>(group_id)
		.execute(conn)
		.await
		.expect("insert group");
	let machine_id = Uuid::new_v4();
	sql_query("INSERT INTO machines (id, group_id, device_id) VALUES ($1, $2, $3)")
		.bind::<sql_types::Uuid, _>(machine_id)
		.bind::<sql_types::Uuid, _>(group_id)
		.bind::<sql_types::Uuid, _>(device_id)
		.execute(conn)
		.await
		.expect("insert machine");
	machine_id
}

async fn application_id(conn: &mut AsyncPgConnection, machine_id: Uuid, key: &str) -> Uuid {
	#[derive(QueryableByName)]
	struct Id {
		#[diesel(sql_type = sql_types::Uuid)]
		id: Uuid,
	}
	let row: Id =
		sql_query("SELECT id FROM applications WHERE machine_id = $1 AND reported_key = $2")
			.bind::<sql_types::Uuid, _>(machine_id)
			.bind::<sql_types::Text, _>(key)
			.get_result(conn)
			.await
			.unwrap_or_else(|e| panic!("no application keyed {key}: {e}"));
	row.id
}

async fn issue_active(
	conn: &mut AsyncPgConnection,
	column: &str,
	id: Uuid,
	r#ref: &str,
) -> Option<bool> {
	let row: Option<Active> = sql_query(format!(
		"SELECT active FROM issues WHERE {column} = $1 AND ref = $2"
	))
	.bind::<sql_types::Uuid, _>(id)
	.bind::<sql_types::Text, _>(r#ref)
	.get_result(conn)
	.await
	.ok();
	row.map(|r| r.active)
}

fn two_workload_push() -> serde_json::Value {
	serde_json::json!({
		"source": "alertd",
		"machine": {
			"health": [{ "check": "disk_free", "result": "failed" }],
			"detail": { "uptime": 4200 },
		},
		"applications": {
			"central": {
				"type": "tamanu-central",
				"health": [{ "check": "db", "result": "passed" }],
				"detail": { "tamanuVersion": "2.4.1" },
			},
			"facility": {
				"type": "tamanu-facility",
				"health": [{ "check": "db", "result": "failed" }],
				"detail": { "tamanuVersion": "2.4.0" },
			},
		},
	})
}

/// The case the split exists for: one box running two workloads reports all
/// three targets in one push, and each one's material lands on it alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_push_describing_two_workloads_files_each_where_the_reporter_put_it() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&two_workload_push())
				.await
				.assert_status_ok();

			let central = application_id(&mut conn, machine_id, "central").await;
			let facility = application_id(&mut conn, machine_id, "facility").await;
			assert_ne!(central, facility);

			// One status row per target: the machine's carries the machine's
			// material and nothing else.
			let machine_row: Row = sql_query(
				"SELECT extra, health FROM statuses WHERE machine_id = $1 AND server_id IS NULL",
			)
			.bind::<sql_types::Uuid, _>(machine_id)
			.get_result(&mut conn)
			.await
			.expect("a machine row");
			assert_eq!(machine_row.extra, serde_json::json!({ "uptime": 4200 }));
			assert_eq!(
				machine_row.health,
				serde_json::json!([{ "check": "disk_free", "result": "failed" }])
			);

			for (id, version, result) in [(central, "2.4.1", "passed"), (facility, "2.4.0", "failed")] {
				let row: Row =
					sql_query("SELECT extra, health FROM statuses WHERE server_id = $1")
						.bind::<sql_types::Uuid, _>(id)
						.get_result(&mut conn)
						.await
						.expect("an application row");
				assert_eq!(row.extra, serde_json::json!({ "tamanuVersion": version }));
				assert_eq!(
					row.health,
					serde_json::json!([{ "check": "db", "result": result }])
				);
			}

			// The failing disk is the box's one issue, not one per workload.
			assert_eq!(
				issue_active(&mut conn, "machine_id", machine_id, "health/disk_free").await,
				Some(true)
			);
			let per_workload: Count = sql_query(
				"SELECT COUNT(*) AS count FROM issues WHERE application_id IN ($1, $2) AND ref = 'health/disk_free'",
			)
			.bind::<sql_types::Uuid, _>(central)
			.bind::<sql_types::Uuid, _>(facility)
			.get_result(&mut conn)
			.await
			.unwrap();
			assert_eq!(per_workload.count, 0);

			// The same check name on both workloads is two independent facts.
			assert_eq!(
				issue_active(&mut conn, "application_id", central, "health/db").await,
				Some(false)
			);
			assert_eq!(
				issue_active(&mut conn, "application_id", facility, "health/db").await,
				Some(true)
			);
		},
	)
	.await
}

/// Detail follows the target it was attached to, with no splitting by field
/// name: the reporter already said which grain each field is.
#[tokio::test(flavor = "multi_thread")]
async fn detail_is_recorded_against_the_target_it_was_attached_to() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "alertd",
					"machine": { "detail": { "uptime": 900 } },
					"applications": {
						"central": {
							"type": "tamanu-central",
							// `uptime` is a machine field by name, and the
							// reporter filed it under the application anyway.
							"detail": { "uptime": 5, "tamanuVersion": "2.4.1" },
						},
					},
				}))
				.await
				.assert_status_ok();

			#[derive(QueryableByName)]
			struct Extra {
				#[diesel(sql_type = sql_types::Jsonb)]
				extra: serde_json::Value,
			}
			let machine_detail: Extra = sql_query(
				"SELECT extra FROM machine_reported_detail WHERE machine_id = $1 AND source = 'alertd'",
			)
			.bind::<sql_types::Uuid, _>(machine_id)
			.get_result(&mut conn)
			.await
			.expect("machine detail");
			assert_eq!(machine_detail.extra, serde_json::json!({ "uptime": 900 }));

			let central = application_id(&mut conn, machine_id, "central").await;
			let app_detail: Extra = sql_query(
				"SELECT extra FROM application_reported_detail WHERE application_id = $1 AND source = 'alertd'",
			)
			.bind::<sql_types::Uuid, _>(central)
			.get_result(&mut conn)
			.await
			.expect("application detail");
			assert_eq!(
				app_detail.extra,
				serde_json::json!({ "uptime": 5, "tamanuVersion": "2.4.1" }),
				"a split push says which grain each field is; Canopy does not second-guess it"
			);
		},
	)
	.await
}

/// A check named like a machine check, reported under an application, is that
/// application's. The reporter separated the grains, so its answer stands over
/// the name.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_named_check_under_an_application_is_the_applications() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "alertd",
					"machine": { "health": [] },
					"applications": {
						"central": {
							"type": "tamanu-central",
							"health": [{ "check": "disk_free", "result": "failed" }],
						},
					},
				}))
				.await
				.assert_status_ok();

			let central = application_id(&mut conn, machine_id, "central").await;
			assert_eq!(
				issue_active(&mut conn, "application_id", central, "health/disk_free").await,
				Some(true)
			);
			assert_eq!(
				issue_active(&mut conn, "machine_id", machine_id, "health/disk_free").await,
				None,
				"nothing files against the box that the box did not report"
			);
		},
	)
	.await
}

/// A push's application section says nothing about the machine's checks. If it
/// did, every filing for a workload would close the box's open issues as
/// unmentioned.
#[tokio::test(flavor = "multi_thread")]
async fn an_applications_filing_leaves_the_machines_checks_alone() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;
			let push = |app_health: serde_json::Value| {
				serde_json::json!({
					"source": "alertd",
					"machine": { "health": [{ "check": "disk_free", "result": "failed" }] },
					"applications": {
						"central": { "type": "tamanu-central", "health": app_health },
					},
				})
			};

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&push(
					serde_json::json!([{ "check": "db", "result": "failed" }]),
				))
				.await
				.assert_status_ok();

			let central = application_id(&mut conn, machine_id, "central").await;
			assert_eq!(
				issue_active(&mut conn, "application_id", central, "health/db").await,
				Some(true)
			);

			// The workload's check goes away; the box's stays exactly as it was.
			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&push(serde_json::json!([])))
				.await
				.assert_status_ok();

			assert_eq!(
				issue_active(&mut conn, "application_id", central, "health/db").await,
				Some(false),
				"a check the source stopped reporting for that target recovered"
			);
			assert_eq!(
				issue_active(&mut conn, "machine_id", machine_id, "health/disk_free").await,
				Some(true),
				"the box's own check is still failing and still says so"
			);
		},
	)
	.await
}

/// The cutover: a box whose application Canopy created from unified pushes is
/// taken over by the first split push naming its type, rather than doubled.
#[tokio::test(flavor = "multi_thread")]
async fn a_split_push_takes_over_the_application_a_unified_push_created() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "alertd",
					"health": [{ "check": "db", "result": "passed" }],
					"tamanuServerKind": "central",
					"tamanuVersion": "2.4.1",
				}))
				.await
				.assert_status_ok();

			#[derive(QueryableByName)]
			struct Id {
				#[diesel(sql_type = sql_types::Uuid)]
				id: Uuid,
			}
			let before: Id = sql_query("SELECT id FROM applications WHERE machine_id = $1")
				.bind::<sql_types::Uuid, _>(machine_id)
				.get_result(&mut conn)
				.await
				.expect("the unified push created one");

			public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "alertd",
					"machine": { "health": [] },
					"applications": {
						"central": {
							"type": "tamanu-central",
							"health": [{ "check": "db", "result": "passed" }],
						},
					},
				}))
				.await
				.assert_status_ok();

			let count: Count =
				sql_query("SELECT COUNT(*) AS count FROM applications WHERE machine_id = $1")
					.bind::<sql_types::Uuid, _>(machine_id)
					.get_result(&mut conn)
					.await
					.unwrap();
			assert_eq!(count.count, 1, "taken over, not doubled");
			assert_eq!(
				application_id(&mut conn, machine_id, "central").await,
				before.id
			);
		},
	)
	.await
}

/// A split push is answered per target, under the keys the reporter used. A
/// unified push is answered exactly as it was before the split shape existed,
/// so nothing changes for a reporter in the field.
#[tokio::test(flavor = "multi_thread")]
async fn each_shape_is_answered_the_way_its_reporter_expects() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			let split: SplitResponse = public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&two_workload_push())
				.await
				.json();

			let machine = split.machine.expect("the box is answered for");
			assert!(machine.check_severities.contains_key("disk_free"));
			assert!(machine.tags.contains_key("canopy:group-id"));

			let applications = split.applications.expect("each workload is answered for");
			let mut keys: Vec<_> = applications.keys().cloned().collect();
			keys.sort();
			assert_eq!(
				keys,
				vec!["central".to_string(), "facility".to_string()],
				"answered under the keys the reporter named them by"
			);
			assert!(applications["central"].check_severities.contains_key("db"));

			let unified: SplitResponse = public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "seedling",
					"health": [{ "check": "db", "result": "passed" }],
					"tamanuServerKind": "central",
				}))
				.await
				.json();
			assert!(unified.machine.is_none());
			assert!(unified.applications.is_none());
		},
	)
	.await
}

/// A box with nothing but itself to report says so by naming the machine and
/// no applications.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_only_push_is_the_boxs_in_full() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			let response: SplitResponse = public
				.post(&format!("/status/{machine_id}"))
				.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
				.json(&serde_json::json!({
					"source": "alertd",
					"machine": {
						"health": [{ "check": "disk_free", "result": "failed" }],
						"detail": { "uptime": 12 },
					},
				}))
				.await
				.json();

			assert!(response.machine.is_some());
			assert_eq!(
				response.applications.map(|a| a.len()),
				Some(0),
				"the reporter named no applications, and is answered about none"
			);

			let count: Count =
				sql_query("SELECT COUNT(*) AS count FROM applications WHERE machine_id = $1")
					.bind::<sql_types::Uuid, _>(machine_id)
					.get_result(&mut conn)
					.await
					.unwrap();
			assert_eq!(count.count, 0, "naming no application creates none");

			assert_eq!(
				issue_active(&mut conn, "machine_id", machine_id, "health/disk_free").await,
				Some(true)
			);
		},
	)
	.await
}

/// The shape is validated per target, and the error says which one is wrong.
#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_target_is_rejected_by_name() {
	commons_tests::server::run_with_device_auth(
		"server",
		async |mut conn, cert, device_id, public, _| {
			let machine_id = machine_for(&mut conn, device_id).await;

			for (body, wanted) in [
				(
					serde_json::json!({
						"machine": { "health": [{ "result": "passed" }] },
					}),
					"machine.health[0].check",
				),
				(
					serde_json::json!({
						"machine": {},
						"applications": { "central": { "health": [] } },
					}),
					"applications.central.type",
				),
				(
					serde_json::json!({
						"machine": {},
						"applications": {
							"central": { "type": "tamanu-central", "detail": 7 },
						},
					}),
					"applications.central.detail",
				),
			] {
				let response = public
					.post(&format!("/status/{machine_id}"))
					.add_header("x-forwarded-client-cert", &format!("Cert={}", cert))
					.json(&body)
					.await;
				response.assert_status(http::StatusCode::BAD_REQUEST);
				assert!(
					response.text().contains(wanted),
					"expected {wanted} in {}",
					response.text()
				);
			}
		},
	)
	.await
}
