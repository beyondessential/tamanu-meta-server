//! Pair derivation and settling for reporting schemas.
//!
//! spec: RPT

use commons_tests::db::TestDb;
use database::{
	diesel_async::AsyncPgConnection,
	reporting_schemas::{
		NewReportingSchemaBuild, PairState, ReportingSchemaBuild, ReportingSchemaRequest,
		pairs_for_group, versions_for_group,
	},
	restore::NewBackupRestoreCheck,
};
use diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

const GROUP: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const MACHINE: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
const CENTRAL: &str = "cccccccc-cccc-cccc-cccc-cccccccccccc";
const FACILITY: &str = "dddddddd-dddd-dddd-dddd-dddddddddddd";
const CONSUMER: &str = "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee";

/// A group with a central on 2.60.0 and a facility on 2.59.0, both published.
async fn seed(conn: &mut AsyncPgConnection) -> (Uuid, Uuid) {
	conn.batch_execute(&format!(
		"INSERT INTO devices (id, role) VALUES ('{CONSUMER}', 'backup-restore');

		 INSERT INTO versions (id, major, minor, patch, changelog, status) VALUES
		 ('11111111-1111-1111-1111-111111111111', 2, 59, 0, '', 'published'),
		 ('22222222-2222-2222-2222-222222222222', 2, 60, 0, '', 'published');

		 INSERT INTO server_groups (id, name) VALUES ('{GROUP}', 'kamaka');

		 INSERT INTO machines (id, name, group_id) VALUES ('{MACHINE}', 'box', '{GROUP}');

		 INSERT INTO applications (id, type, name, host, machine_id, group_id) VALUES
		 ('{CENTRAL}', 'tamanu-central', 'central', 'https://c', '{MACHINE}', '{GROUP}'),
		 ('{FACILITY}', 'tamanu-facility', 'facility', 'https://f', '{MACHINE}', '{GROUP}');

		 INSERT INTO application_reported_detail (application_id, source, reported_at, version) VALUES
		 ('{CENTRAL}', 'tamanu', NOW(), '2.60.0'),
		 ('{FACILITY}', 'tamanu', NOW(), '2.59.0')",
	))
	.await
	.expect("seed");

	(
		"11111111-1111-1111-1111-111111111111".parse().unwrap(),
		"22222222-2222-2222-2222-222222222222".parse().unwrap(),
	)
}

fn group() -> Uuid {
	GROUP.parse().unwrap()
}

/// A restore report for the pair, as a consumer would send one.
fn report_for(
	outcome: commons_types::backup::RunOutcome,
	healthy: bool,
	error: Option<String>,
) -> NewBackupRestoreCheck {
	NewBackupRestoreCheck {
		replica_id: None,
		replica_name: None,
		consumer_device_id: CONSUMER.parse().unwrap(),
		group_id: group(),
		machine_id: Some(MACHINE.parse().unwrap()),
		r#type: "tamanu-postgres".parse().unwrap(),
		intent: "reporting-schema".parse().unwrap(),
		snapshot_id: Some("snap-1".to_owned()),
		outcome,
		error,
		replica_healthy: healthy,
		postgres_version: None,
		observed_at: jiff::Timestamp::now(),
		s3_sent_raw_bytes: None,
		s3_sent_payload_bytes: None,
		s3_received_raw_bytes: None,
		s3_received_payload_bytes: None,
		health_details: None,
		run_id: None,
		redaction_outcome: None,
		redaction_manifest_version: None,
		redaction_columns_masked: None,
		redaction_columns_skipped: None,
		redaction_error: None,
	}
}

/// Record a build against a throwaway restore report for the pair.
async fn record_build(conn: &mut AsyncPgConnection, version: Uuid, built: bool) {
	let report = NewBackupRestoreCheck {
		replica_id: None,
		replica_name: None,
		consumer_device_id: CONSUMER.parse().unwrap(),
		group_id: group(),
		machine_id: Some(MACHINE.parse().unwrap()),
		r#type: "tamanu-postgres".parse().unwrap(),
		intent: "reporting-schema".parse().unwrap(),
		snapshot_id: Some("snap-1".to_owned()),
		outcome: commons_types::backup::RunOutcome::Success,
		error: None,
		replica_healthy: true,
		postgres_version: None,
		observed_at: jiff::Timestamp::now(),
		s3_sent_raw_bytes: None,
		s3_sent_payload_bytes: None,
		s3_received_raw_bytes: None,
		s3_received_payload_bytes: None,
		health_details: None,
		run_id: None,
		redaction_outcome: None,
		redaction_manifest_version: None,
		redaction_columns_masked: None,
		redaction_columns_skipped: None,
		redaction_error: None,
	};

	ReportingSchemaBuild::record(
		conn,
		report,
		NewReportingSchemaBuild {
			group_id: group(),
			version_id: version,
			application_id: Some(CENTRAL.parse().unwrap()),
			built,
			error: (!built).then(|| "views did not compile".to_owned()),
			artifact_ids: vec![],
		},
	)
	.await
	.expect("record build");
}

/// The pairs are every version the group's Tamanu applications report running.
/// A facility mid-rollout is on a different version from its central, and both
/// are pairs, because a schema follows the version rather than the application.
#[tokio::test(flavor = "multi_thread")]
async fn a_facility_on_its_own_version_is_a_pair_of_its_own() {
	TestDb::run(|mut conn, _url| async move {
		let (older, newer) = seed(&mut conn).await;

		let versions = versions_for_group(&mut conn, group())
			.await
			.expect("derive versions");
		let ids: Vec<Uuid> = versions.iter().map(|v| v.id).collect();

		assert!(ids.contains(&older), "the facility's version is a pair");
		assert!(ids.contains(&newer), "the central's version is a pair");
	})
	.await;
}

/// A pair with no build is awaiting one; a built pair is settled; a failed
/// build settles it as firmly, since a build against a fixed version and
/// configuration fails the same way every time.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_build_settles_the_pair() {
	TestDb::run(|mut conn, _url| async move {
		let (_older, newer) = seed(&mut conn).await;

		assert!(
			!ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"an untried pair is on the worklist"
		);

		record_build(&mut conn, newer, false).await;

		assert!(
			ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"a failure settles it as firmly as a pass"
		);

		let pairs = pairs_for_group(&mut conn, group()).await.expect("pairs");
		let failed = pairs.iter().find(|p| p.version_id == newer).unwrap();
		assert_eq!(failed.state, PairState::Failed);
		assert_eq!(failed.error.as_deref(), Some("views did not compile"));
	})
	.await;
}

/// An operator asking for a build reinstates a settled pair, and the ask is
/// answered once the build lands.
#[tokio::test(flavor = "multi_thread")]
async fn an_operator_ask_reinstates_a_settled_pair() {
	TestDb::run(|mut conn, _url| async move {
		let (_older, newer) = seed(&mut conn).await;

		record_build(&mut conn, newer, true).await;
		assert!(
			ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap()
		);

		ReportingSchemaRequest::enqueue(&mut conn, group(), newer, Some("someone@bes.au"))
			.await
			.expect("enqueue");

		assert!(
			!ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"an ask puts the pair back on the worklist"
		);

		let pairs = pairs_for_group(&mut conn, group()).await.expect("pairs");
		assert!(
			pairs
				.iter()
				.find(|p| p.version_id == newer)
				.unwrap()
				.requested
		);

		// The build that answers the ask clears it.
		record_build(&mut conn, newer, true).await;
		assert!(
			ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"the ask is answered once the build it asked for lands"
		);
	})
	.await;
}

/// A replica that failed to restore says nothing about whether the pair can be
/// built, so it records no build and the pair stays on the worklist.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_restore_leaves_the_pair_unsettled() {
	TestDb::run(|mut conn, _url| async move {
		let (_older, newer) = seed(&mut conn).await;

		let report = NewBackupRestoreCheck {
			replica_id: None,
			replica_name: None,
			consumer_device_id: CONSUMER.parse().unwrap(),
			group_id: group(),
			machine_id: Some(MACHINE.parse().unwrap()),
			r#type: "tamanu-postgres".parse().unwrap(),
			intent: "reporting-schema".parse().unwrap(),
			snapshot_id: Some("snap-1".to_owned()),
			outcome: commons_types::backup::RunOutcome::Failure,
			error: Some("replica never came up".to_owned()),
			replica_healthy: false,
			postgres_version: None,
			observed_at: jiff::Timestamp::now(),
			s3_sent_raw_bytes: None,
			s3_sent_payload_bytes: None,
			s3_received_raw_bytes: None,
			s3_received_payload_bytes: None,
			health_details: None,
			run_id: None,
			redaction_outcome: None,
			redaction_manifest_version: None,
			redaction_columns_masked: None,
			redaction_columns_skipped: None,
			redaction_error: None,
		};

		ReportingSchemaBuild::record(
			&mut conn,
			report,
			NewReportingSchemaBuild {
				group_id: group(),
				version_id: newer,
				application_id: Some(CENTRAL.parse().unwrap()),
				built: false,
				error: None,
				artifact_ids: vec![],
			},
		)
		.await
		.expect("record");

		assert!(
			!ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"an unhealthy restore is dispatched again rather than settling the pair"
		);
	})
	.await;
}

/// A schema built from a superseded release of a version is not the schema that
/// version describes, so registering an artifact against the version puts the
/// pair back on the worklist without an operator asking.
#[tokio::test(flavor = "multi_thread")]
async fn a_new_artifact_for_the_version_reinstates_the_pair() {
	TestDb::run(|mut conn, _url| async move {
		let (_older, newer) = seed(&mut conn).await;

		record_build(&mut conn, newer, true).await;
		assert!(
			ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"a built pair settles"
		);

		conn.batch_execute(&format!(
			"INSERT INTO artifacts (version_id, artifact_type, platform, download_url)
			 VALUES ('{}', 'migrations', 'any', 'https://example.com/m.tar')",
			newer
		))
		.await
		.expect("register an artifact for the version");

		assert!(
			!ReportingSchemaBuild::is_settled(&mut conn, group(), newer)
				.await
				.unwrap(),
			"the version's artifacts changed, so the pair is built again"
		);
	})
	.await;
}

/// A build records the artifacts it registered, so an operator can see what came
/// out of it rather than only that something did.
#[tokio::test(flavor = "multi_thread")]
async fn a_build_records_what_it_registered() {
	TestDb::run(|mut conn, _url| async move {
		let (_older, newer) = seed(&mut conn).await;

		let artifact: Uuid = "77777777-7777-7777-7777-777777777777".parse().unwrap();
		conn.batch_execute(&format!(
			"INSERT INTO artifacts (id, version_id, artifact_type, platform, download_url)
			 VALUES ('{artifact}', '{newer}', 'reporting-schema', 'any', 'https://example.com/s.sql')"
		))
		.await
		.expect("seed artifact");

		let report = report_for(commons_types::backup::RunOutcome::Success, true, None);
		ReportingSchemaBuild::record(
			&mut conn,
			report,
			NewReportingSchemaBuild {
				group_id: group(),
				version_id: newer,
				application_id: Some(CENTRAL.parse().unwrap()),
				built: true,
				error: None,
				artifact_ids: vec![artifact],
			},
		)
		.await
		.expect("record");

		let build = ReportingSchemaBuild::latest_for_pair(&mut conn, group(), newer)
			.await
			.unwrap()
			.expect("a build");
		assert_eq!(build.artifact_ids, vec![Some(artifact)]);
	})
	.await;
}
