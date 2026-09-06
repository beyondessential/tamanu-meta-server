//! DB-layer tests for group-scoped artifact resolution (`database::artifacts`).
//!
//! spec: ART

use commons_tests::db::TestDb;
use database::{
	artifacts::{Artifact, NewArtifact, Scope, digest_of},
	diesel_async::AsyncPgConnection,
};
use diesel_async::SimpleAsyncConnection;
use uuid::Uuid;

async fn seed_version(conn: &mut AsyncPgConnection, major: i32, minor: i32, patch: i32) -> Uuid {
	conn.batch_execute(&format!(
		"INSERT INTO versions (major, minor, patch, changelog, status) \
		 VALUES ({major}, {minor}, {patch}, '', 'published')"
	))
	.await
	.expect("seed version");

	let version = database::versions::Version::get_by_version(
		conn,
		commons_types::version::VersionStr(node_semver::Version {
			major: major as u64,
			minor: minor as u64,
			patch: patch as u64,
			build: vec![],
			pre_release: vec![],
		}),
	)
	.await
	.expect("read back version");
	version.id
}

async fn seed_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (name) VALUES ('{name}')"
	))
	.await
	.expect("seed group");

	let groups = database::server_groups::ServerGroup::list_all(conn)
		.await
		.expect("list groups");
	groups
		.into_iter()
		.find(|g| g.name == name)
		.expect("group is there")
		.id
}

fn unscoped(version_id: Uuid, artifact_type: &str, url: &str) -> NewArtifact {
	NewArtifact {
		version_id: Some(version_id),
		artifact_type: artifact_type.to_owned(),
		platform: "any".to_owned(),
		download_url: Some(url.to_owned()),
		device_id: None,
		version_range_pattern: None,
		group_id: None,
		content: None,
		content_type: None,
		digest: None,
		run_id: None,
	}
}

fn held(version_id: Uuid, artifact_type: &str, group: Uuid, bytes: &[u8]) -> NewArtifact {
	NewArtifact {
		version_id: Some(version_id),
		artifact_type: artifact_type.to_owned(),
		platform: "any".to_owned(),
		download_url: None,
		device_id: None,
		version_range_pattern: None,
		group_id: Some(group),
		content: Some(bytes.to_vec()),
		content_type: Some("application/sql".to_owned()),
		digest: Some(digest_of(bytes)),
		run_id: None,
	}
}

/// A group-scoped artifact and an unscoped one of the same type and platform
/// are both recorded, each group is offered the one for it, and no caller is
/// offered both.
#[tokio::test(flavor = "multi_thread")]
async fn each_group_is_offered_its_own_and_never_both() {
	TestDb::run(|mut conn, _url| async move {
		let version = seed_version(&mut conn, 2, 60, 0).await;
		let theirs = seed_group(&mut conn, "kamaka").await;
		let other = seed_group(&mut conn, "drifting").await;

		Artifact::register(&mut conn, unscoped(version, "reporting-schema", "https://x/y"))
			.await
			.expect("register unscoped");
		Artifact::register(&mut conn, held(version, "reporting-schema", theirs, b"theirs"))
			.await
			.expect("register held");

		let offered = Artifact::get_for_version(&mut conn, version, Scope::Group(theirs))
			.await
			.expect("resolve for the owning group");
		assert_eq!(offered.len(), 1, "never offered both");
		assert_eq!(offered[0].group_id, Some(theirs));

		// Another group's read reaches the unscoped one, not the first group's.
		let offered = Artifact::get_for_version(&mut conn, version, Scope::Group(other))
			.await
			.expect("resolve for another group");
		assert_eq!(offered.len(), 1);
		assert_eq!(offered[0].group_id, None);

		// A read carrying no identity is answered with the unscoped set alone.
		let offered = Artifact::get_for_version(&mut conn, version, Scope::Unscoped)
			.await
			.expect("resolve anonymously");
		assert_eq!(offered.len(), 1);
		assert_eq!(offered[0].group_id, None);

		// An operator sees what resolution passed over.
		let all = Artifact::get_for_version_all_matches(&mut conn, version, Scope::Fleet)
			.await
			.expect("operator view");
		assert_eq!(all.len(), 2);
	})
	.await;
}

/// A group-scoped artifact is more specific than an unscoped one, so it wins
/// even where the unscoped one is exact and it is only a range match.
#[tokio::test(flavor = "multi_thread")]
async fn group_scope_outranks_an_exact_unscoped_artifact() {
	TestDb::run(|mut conn, _url| async move {
		let version = seed_version(&mut conn, 2, 60, 0).await;
		let theirs = seed_group(&mut conn, "kamaka").await;

		Artifact::register(&mut conn, unscoped(version, "installer", "https://x/exact"))
			.await
			.expect("register exact unscoped");

		let mut ranged = held(version, "installer", theirs, b"ranged");
		ranged.version_id = None;
		ranged.version_range_pattern = Some("2.60.x".to_owned());
		Artifact::register(&mut conn, ranged)
			.await
			.expect("register ranged held");

		let offered = Artifact::get_for_version(&mut conn, version, Scope::Group(theirs))
			.await
			.expect("resolve");
		assert_eq!(offered.len(), 1);
		assert_eq!(
			offered[0].group_id,
			Some(theirs),
			"the group's range artifact beats an exact artifact for everyone"
		);
	})
	.await;
}

/// A registration replaces whatever is already registered for the same version,
/// type, platform and group, and the bytes it replaces do not survive.
#[tokio::test(flavor = "multi_thread")]
async fn registering_again_replaces_the_bytes_it_held() {
	TestDb::run(|mut conn, _url| async move {
		let version = seed_version(&mut conn, 2, 60, 0).await;
		let theirs = seed_group(&mut conn, "kamaka").await;

		let first = Artifact::register(
			&mut conn,
			held(version, "reporting-schema", theirs, b"first build"),
		)
		.await
		.expect("first registration");

		let second = Artifact::register(
			&mut conn,
			held(version, "reporting-schema", theirs, b"second build"),
		)
		.await
		.expect("second registration");

		assert_eq!(first.id, second.id, "replaced in place, not duplicated");

		let all = Artifact::get_for_version_all_matches(&mut conn, version, Scope::Fleet)
			.await
			.expect("operator view");
		assert_eq!(all.len(), 1, "a caller is never offered two of a kind");

		let content = Artifact::content_for(&mut conn, second.id)
			.await
			.expect("read content")
			.expect("bytes are held");
		assert_eq!(content.bytes, b"second build");
		assert_eq!(content.digest, digest_of(b"second build"));
	})
	.await;
}

/// The same type and platform can be registered for one group and for another
/// without colliding, which the old version-only unique constraint could not
/// express.
#[tokio::test(flavor = "multi_thread")]
async fn two_groups_hold_their_own_of_the_same_kind() {
	TestDb::run(|mut conn, _url| async move {
		let version = seed_version(&mut conn, 2, 60, 0).await;
		let one = seed_group(&mut conn, "kamaka").await;
		let two = seed_group(&mut conn, "drifting").await;

		Artifact::register(&mut conn, held(version, "reporting-schema", one, b"one"))
			.await
			.expect("first group");
		Artifact::register(&mut conn, held(version, "reporting-schema", two, b"two"))
			.await
			.expect("second group");

		let all = Artifact::get_for_version_all_matches(&mut conn, version, Scope::Fleet)
			.await
			.expect("operator view");
		assert_eq!(all.len(), 2);
	})
	.await;
}

/// A range artifact registered twice replaces itself. Before the identity
/// index this could not hold: `version_id` is NULL for every range artifact,
/// and the default treatment of NULL made each row distinct from the last.
#[tokio::test(flavor = "multi_thread")]
async fn a_range_artifact_replaces_itself() {
	TestDb::run(|mut conn, _url| async move {
		let version = seed_version(&mut conn, 2, 60, 0).await;

		for url in ["https://x/first", "https://x/second"] {
			let mut ranged = unscoped(version, "installer", url);
			ranged.version_id = None;
			ranged.version_range_pattern = Some("2.60.x".to_owned());
			Artifact::register(&mut conn, ranged)
				.await
				.expect("register range artifact");
		}

		let all = Artifact::get_for_version_all_matches(&mut conn, version, Scope::Fleet)
			.await
			.expect("operator view");
		assert_eq!(all.len(), 1, "the second registration replaced the first");
		assert_eq!(all[0].download_url.as_deref(), Some("https://x/second"));
	})
	.await;
}
