//! The interlock between passphrase rotation and device backups.
//!
//! `kopia change-password` rewrites the repository's format blob, so the
//! passphrase a device is holding stops working the moment it lands. Devices
//! get that passphrase from `GET /backup-target` together with credentials
//! good for an hour, and nothing on that path touches the worker's per-group
//! slot — that only excludes maintenance, inspection and init, and only within
//! one process.

use commons_tests::db::TestDb;
use commons_types::backup::{BackupConfigStatus, BackupPlacement, BackupRepoMode, BackupType};
use database::backups::{
	BackupCredentialIssuance, NewBackupCredentialIssuance, NewServerGroupBackupConfig,
	ROTATION_WINDOW, ServerGroupBackupConfig,
};
use database::diesel_async::AsyncPgConnection;
use diesel_async::SimpleAsyncConnection;
use jiff::{SignedDuration, Timestamp};
use uuid::Uuid;

async fn seed(conn: &mut AsyncPgConnection) -> (Uuid, Uuid) {
	let group_id = Uuid::new_v4();
	let device_id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{group_id}', 'Rot'); \
		 INSERT INTO devices (id, role) VALUES ('{device_id}', 'machine');"
	))
	.await
	.expect("seed group + device");

	ServerGroupBackupConfig::upsert(
		conn,
		NewServerGroupBackupConfig {
			group_id,
			bucket: "b".into(),
			prefix: String::new(),
			target_role_arn: "arn:aws:iam::123456789012:role/t".into(),
			maintenance_role_arn: "arn:aws:iam::123456789012:role/m".into(),
			region: None,
			repo_password_ref: "pw".into(),
			status: BackupConfigStatus::Provisioning,
			mode: BackupRepoMode::FromBirth,
			placement: BackupPlacement::External,
		},
	)
	.await
	.expect("insert config");
	(group_id, device_id)
}

async fn issue(conn: &mut AsyncPgConnection, group_id: Uuid, device_id: Uuid, ttl: SignedDuration) {
	let now = Timestamp::now();
	BackupCredentialIssuance::record(
		conn,
		NewBackupCredentialIssuance {
			device_id,
			group_id,
			r#type: BackupType::from("tamanu-postgres"),
			expires_at: now + ttl,
			purpose: commons_types::backup::BackupPurpose::Backup,
			sts_assumed_role: "arn:aws:iam::123456789012:role/t".into(),
			sts_request_id: None,
			access_key_id: None,
			bucket: "b".into(),
			prefix: String::new(),
			run_id: None,
		},
	)
	.await
	.expect("record issuance");
}

/// The condition rotation must not run through: a device was handed the
/// current passphrase and its credentials are still good, so it may be
/// mid-backup right now.
#[tokio::test(flavor = "multi_thread")]
async fn live_credentials_are_visible_to_the_rotation_check() {
	TestDb::run(|mut conn, _url| async move {
		let (group_id, device_id) = seed(&mut conn).await;
		let now = Timestamp::now();

		assert!(
			!BackupCredentialIssuance::any_live_for_group(&mut conn, group_id, now)
				.await
				.expect("query"),
			"nothing issued yet",
		);

		issue(
			&mut conn,
			group_id,
			device_id,
			SignedDuration::from_hours(1),
		)
		.await;
		assert!(
			BackupCredentialIssuance::any_live_for_group(&mut conn, group_id, now)
				.await
				.expect("query"),
			"an unexpired issuance means a backup may be running",
		);

		// Once they expire the device can no longer be mid-run with them.
		assert!(
			!BackupCredentialIssuance::any_live_for_group(
				&mut conn,
				group_id,
				now + SignedDuration::from_hours(2)
			)
			.await
			.expect("query"),
		);
	})
	.await;
}

/// Another group's credentials must not defer this group's rotation.
#[tokio::test(flavor = "multi_thread")]
async fn live_credentials_are_scoped_to_their_group() {
	TestDb::run(|mut conn, _url| async move {
		let (group_a, device_a) = seed(&mut conn).await;
		let (group_b, _) = seed(&mut conn).await;
		issue(&mut conn, group_a, device_a, SignedDuration::from_hours(1)).await;

		let now = Timestamp::now();
		assert!(
			BackupCredentialIssuance::any_live_for_group(&mut conn, group_a, now)
				.await
				.expect("query")
		);
		assert!(
			!BackupCredentialIssuance::any_live_for_group(&mut conn, group_b, now)
				.await
				.expect("query")
		);
	})
	.await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_interlock_is_exclusive_and_releasable() {
	TestDb::run(|mut conn, _url| async move {
		let (group_id, _) = seed(&mut conn).await;
		let t0 = Timestamp::now();

		assert!(
			!ServerGroupBackupConfig::passphrase_rotation_in_flight(&mut conn, group_id, t0)
				.await
				.expect("query"),
			"not rotating to begin with",
		);

		assert!(
			ServerGroupBackupConfig::begin_passphrase_rotation(&mut conn, group_id, t0)
				.await
				.expect("claim"),
			"first claim succeeds",
		);
		assert!(
			ServerGroupBackupConfig::passphrase_rotation_in_flight(&mut conn, group_id, t0)
				.await
				.expect("query"),
			"issuance is now refused",
		);
		assert!(
			!ServerGroupBackupConfig::begin_passphrase_rotation(&mut conn, group_id, t0)
				.await
				.expect("claim"),
			"a second rotation must not run concurrently",
		);

		ServerGroupBackupConfig::end_passphrase_rotation(&mut conn, group_id)
			.await
			.expect("release");
		assert!(
			!ServerGroupBackupConfig::passphrase_rotation_in_flight(&mut conn, group_id, t0)
				.await
				.expect("query"),
			"issuance resumes once the rotation finishes",
		);
	})
	.await;
}

/// A rotation whose process dies without releasing must not block the group's
/// backups forever — the marker is timestamped precisely so it can expire.
#[tokio::test(flavor = "multi_thread")]
async fn a_crashed_rotation_stops_blocking_once_its_marker_ages_out() {
	TestDb::run(|mut conn, _url| async move {
		let (group_id, _) = seed(&mut conn).await;
		let t0 = Timestamp::now();
		assert!(
			ServerGroupBackupConfig::begin_passphrase_rotation(&mut conn, group_id, t0)
				.await
				.expect("claim")
		);

		let later = t0 + ROTATION_WINDOW + SignedDuration::from_mins(1);
		assert!(
			!ServerGroupBackupConfig::passphrase_rotation_in_flight(&mut conn, group_id, later)
				.await
				.expect("query"),
			"a stale marker must not keep refusing credentials",
		);
		assert!(
			ServerGroupBackupConfig::begin_passphrase_rotation(&mut conn, group_id, later)
				.await
				.expect("claim"),
			"and a later rotation can take the interlock over",
		);
	})
	.await;
}
