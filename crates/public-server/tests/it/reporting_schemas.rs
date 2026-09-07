//! Dispatching reporting-schema builds, and who may publish what one produces.
//!
//! spec: RPT

use axum::http::StatusCode;
use diesel_async::SimpleAsyncConnection;

const GROUP: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const OTHER_GROUP: &str = "ffffffff-ffff-ffff-ffff-ffffffffffff";
const MACHINE: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
const CENTRAL: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";
const VERSION: &str = "22222222-2222-2222-2222-222222222222";

/// A group whose central runs 2.60.0, with a ready backup repo and a snapshot,
/// and a consumer device declared against it building reporting schemas.
async fn seed(conn: &mut database::diesel_async::AsyncPgConnection, consumer: uuid::Uuid) {
	conn.batch_execute(&format!(
		"INSERT INTO versions (id, major, minor, patch, changelog, status)
		 VALUES ('{VERSION}', 2, 60, 0, '', 'published');

		 INSERT INTO server_groups (id, name) VALUES
		 ('{GROUP}', 'kamaka'), ('{OTHER_GROUP}', 'drifting');

		 INSERT INTO machines (id, name, group_id) VALUES ('{MACHINE}', 'box', '{GROUP}');

		 INSERT INTO applications (id, type, name, host, machine_id, group_id)
		 VALUES ('{CENTRAL}', 'tamanu-central', 'central', 'https://c', '{MACHINE}', '{GROUP}');

		 INSERT INTO application_reported_detail (application_id, source, reported_at, version)
		 VALUES ('{CENTRAL}', 'tamanu', NOW(), '2.60.0');

		 INSERT INTO server_group_backup_config
		   (group_id, bucket, prefix, target_role_arn, maintenance_role_arn, repo_password_ref, status)
		 VALUES ('{GROUP}', 'b', 'p/', 'arn:t', 'arn:m', 'ref', 'ready');

		 INSERT INTO backup_runs
		   (id, device_id, machine_id, group_id, type, purpose, outcome, snapshot_id, reported_at)
		 VALUES (gen_random_uuid(), '{consumer}', '{MACHINE}', '{GROUP}', 'tamanu-postgres', 'backup', 'success', 'snap-1', NOW());

		 INSERT INTO restore_consumer_capabilities
		   (consumer_device_id, intent, description, semantics, params)
		 VALUES ('{consumer}', 'schema-build', 'builds schemas',
		         '[\"check\", \"once\", \"migrate\", \"reporting-schema\"]'::jsonb, '{{}}'::jsonb);

		 INSERT INTO restore_replicas
		   (consumer_device_id, group_id, type, intent, name, enabled)
		 VALUES ('{consumer}', '{GROUP}', 'tamanu-postgres', 'schema-build', 'schemas', true)",
	))
	.await
	.expect("seed");
}

/// A build is dispatched per pair on the group's central machine, naming the
/// pair's version rather than the machine's own upgrade candidate.
#[tokio::test(flavor = "multi_thread")]
async fn a_build_is_dispatched_per_pair_on_the_central() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn, device_id).await;

			let response = public
				.get("/restore-worklist")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;
			response.assert_status_ok();
			let entries: Vec<serde_json::Value> = response.json();

			let ours: Vec<&serde_json::Value> = entries
				.iter()
				.filter(|e| e["intent"] == "schema-build")
				.collect();

			assert_eq!(ours.len(), 1, "one entry for the group's one pair");
			assert_eq!(ours[0]["machine_id"], MACHINE, "restores the central's box");
			assert_eq!(
				ours[0]["target_version"], "2.60.0",
				"names the pair's version"
			);
			assert_eq!(ours[0]["application_type"], "tamanu-central");
		},
	)
	.await
}

/// `once` is keyed to the pair rather than the snapshot, so a pair that has been
/// built drops off the worklist and stays off while the snapshot moves on.
#[tokio::test(flavor = "multi_thread")]
async fn a_built_pair_drops_off_the_worklist() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn, device_id).await;

			// Record a build for the pair, riding a restore report as one does.
			conn.batch_execute(&format!(
				"INSERT INTO backup_restore_checks
				   (consumer_device_id, group_id, machine_id, type, intent, snapshot_id,
				    outcome, replica_healthy, observed_at, reported_at)
				 VALUES ('{device_id}', '{GROUP}', '{MACHINE}', 'tamanu-postgres',
				         'schema-build', 'snap-1', 'success', true, NOW(), NOW());

				 INSERT INTO reporting_schema_builds (check_id, group_id, version_id, built)
				 SELECT id, '{GROUP}', '{VERSION}', true FROM backup_restore_checks
				 ORDER BY id DESC LIMIT 1",
			))
			.await
			.expect("record a build");

			let response = public
				.get("/restore-worklist")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;
			response.assert_status_ok();
			let entries: Vec<serde_json::Value> = response.json();

			assert!(
				!entries.iter().any(|e| e["intent"] == "schema-build"),
				"a built pair is settled and not dispatched again"
			);

			// A newer snapshot does not bring it back: the key is the pair.
			conn.batch_execute(&format!(
				"INSERT INTO backup_runs
				   (id, device_id, machine_id, group_id, type, purpose, outcome, snapshot_id, reported_at)
				 VALUES (gen_random_uuid(), '{device_id}', '{MACHINE}', '{GROUP}', 'tamanu-postgres', 'backup', 'success', 'snap-2', NOW())",
			))
			.await
			.expect("newer snapshot");

			let response = public
				.get("/restore-worklist")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;
			let entries: Vec<serde_json::Value> = response.json();
			assert!(
				!entries.iter().any(|e| e["intent"] == "schema-build"),
				"a newer snapshot does not rebuild a schema the pair already has"
			);
		},
	)
	.await
}

/// A builder registers artifacts for the group its declaration covers, and is
/// refused another's the same way it would be refused a group that does not
/// exist.
#[tokio::test(flavor = "multi_thread")]
async fn a_builder_publishes_only_for_its_own_group() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn, device_id).await;

			let ours = public
				.post(&format!(
					"/artifacts/2.60.0/reporting-schema/any?group={GROUP}"
				))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.add_header("content-type", "application/sql")
				.text("CREATE VIEW ...")
				.await;
			ours.assert_status_ok();

			let theirs = public
				.post(&format!(
					"/artifacts/2.60.0/reporting-schema/any?group={OTHER_GROUP}"
				))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.text("CREATE VIEW ...")
				.await;
			assert_eq!(theirs.status_code(), StatusCode::FORBIDDEN);

			let nowhere = public
				.post(
					"/artifacts/2.60.0/reporting-schema/any?group=99999999-9999-9999-9999-999999999999",
				)
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.text("CREATE VIEW ...")
				.await;
			assert_eq!(
				nowhere.status_code(),
				theirs.status_code(),
				"a group it is not authorised for and one that does not exist answer alike"
			);
		},
	)
	.await
}

/// A declaration an operator has turned off does not authorise anything. It is
/// the enabled declaration that covers a group, so a builder whose declaration
/// is disabled is refused its own group's artifacts.
#[tokio::test(flavor = "multi_thread")]
async fn a_disabled_declaration_authorises_nothing() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn, device_id).await;

			conn.batch_execute(&format!(
				"UPDATE restore_replicas SET enabled = false WHERE consumer_device_id = '{device_id}'"
			))
			.await
			.expect("disable the declaration");

			let refused = public
				.post(&format!(
					"/artifacts/2.60.0/reporting-schema/any?group={GROUP}"
				))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.add_header("content-type", "application/sql")
				.text("CREATE VIEW ...")
				.await;

			assert_eq!(refused.status_code(), StatusCode::FORBIDDEN);
		},
	)
	.await
}

/// Restoring for a group is not the same authority as building its schema. A
/// consumer whose declaration covers the group but whose intent advertises no
/// `reporting-schema` semantic is refused, so a verify or migrate consumer
/// cannot publish a schema for the group it already restores.
#[tokio::test(flavor = "multi_thread")]
async fn restoring_for_a_group_does_not_authorise_publishing_its_schema() {
	commons_tests::server::run_with_device_auth(
		"backup-restore",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn, device_id).await;

			conn.batch_execute(&format!(
				"UPDATE restore_consumer_capabilities
				 SET semantics = '[\"check\", \"once\", \"migrate\"]'::jsonb
				 WHERE consumer_device_id = '{device_id}'"
			))
			.await
			.expect("withdraw the semantic");

			let refused = public
				.post(&format!(
					"/artifacts/2.60.0/reporting-schema/any?group={GROUP}"
				))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.add_header("content-type", "application/sql")
				.text("CREATE VIEW ...")
				.await;

			assert_eq!(refused.status_code(), StatusCode::FORBIDDEN);
		},
	)
	.await
}
