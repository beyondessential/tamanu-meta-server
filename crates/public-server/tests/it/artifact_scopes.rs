//! A group-scoped artifact reaches its own group's machines and nobody else.
//!
//! spec: ART

use axum::http::StatusCode;
use database::artifacts::digest_of;
use diesel_async::SimpleAsyncConnection;

const VERSION: &str = "11111111-1111-1111-1111-111111111111";
const UNSCOPED: &str = "22222222-2222-2222-2222-222222222222";
const THEIRS: &str = "33333333-3333-3333-3333-333333333333";
const GROUP_A: &str = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
const GROUP_B: &str = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

/// One published version, two groups, and a `reporting-schema` artifact for
/// each of the unscoped and group-A cases.
async fn seed(conn: &mut database::diesel_async::AsyncPgConnection) {
	let digest = digest_of(b"group a schema");
	conn.batch_execute(&format!(
		"INSERT INTO versions (id, major, minor, patch, changelog, status)
		 VALUES ('{VERSION}', 2, 60, 0, '', 'published');

		 INSERT INTO server_groups (id, name) VALUES
		 ('{GROUP_A}', 'kamaka'), ('{GROUP_B}', 'drifting');

		 INSERT INTO artifacts (id, version_id, platform, artifact_type, download_url)
		 VALUES ('{UNSCOPED}', '{VERSION}', 'any', 'reporting-schema', 'https://example.com/all.sql');

		 INSERT INTO artifacts (id, version_id, platform, artifact_type, group_id, content, content_type, digest)
		 VALUES ('{THEIRS}', '{VERSION}', 'any', 'reporting-schema', '{GROUP_A}',
		         'group a schema'::bytea, 'application/sql', '{digest}')",
	))
	.await
	.expect("seed");
}

/// Put the authenticated device on a machine in the given group.
async fn enrol(
	conn: &mut database::diesel_async::AsyncPgConnection,
	device_id: uuid::Uuid,
	group: &str,
) {
	conn.batch_execute(&format!(
		"INSERT INTO machines (name, group_id, device_id)
		 VALUES ('box', '{group}', '{device_id}')"
	))
	.await
	.expect("enrol machine");
}

/// A read carrying no identity is answered with the unscoped artifacts alone,
/// so giving an artifact a group narrows who is offered it rather than
/// widening what an open path serves.
#[tokio::test(flavor = "multi_thread")]
async fn an_anonymous_read_sees_only_unscoped_artifacts() {
	commons_tests::server::run(async |mut conn, public, _| {
		seed(&mut conn).await;

		let response = public.get("/versions/2.60.0/artifacts").await;
		response.assert_status_ok();
		let artifacts: Vec<serde_json::Value> = response.json();

		assert_eq!(artifacts.len(), 1);
		assert_eq!(artifacts[0]["id"], UNSCOPED);
		assert!(artifacts[0]["group_id"].is_null());
	})
	.await
}

/// A caller whose credential is bound to a machine has that machine's group,
/// and the artifact scoped to it displaces the unscoped one of the same type
/// and platform.
#[tokio::test(flavor = "multi_thread")]
async fn a_machine_is_offered_its_own_group_s_artifact() {
	commons_tests::server::run_with_device_auth(
		"machine",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn).await;
			enrol(&mut conn, device_id, GROUP_A).await;

			let response = public
				.get("/versions/2.60.0/artifacts")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;
			response.assert_status_ok();
			let artifacts: Vec<serde_json::Value> = response.json();

			assert_eq!(artifacts.len(), 1, "never offered both");
			assert_eq!(artifacts[0]["id"], THEIRS);
			assert_eq!(artifacts[0]["group_id"], GROUP_A);
		},
	)
	.await
}

/// A machine in another group reaches the unscoped artifact, and the one it is
/// not offered is answered as though it did not exist. The refusal is the same
/// one an artifact id that was never registered gets, so which groups hold an
/// artifact is not enumerable here.
#[tokio::test(flavor = "multi_thread")]
async fn another_group_cannot_tell_the_artifact_apart_from_a_missing_one() {
	commons_tests::server::run_with_device_auth(
		"machine",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn).await;
			enrol(&mut conn, device_id, GROUP_B).await;

			let response = public
				.get("/versions/2.60.0/artifacts")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;
			response.assert_status_ok();
			let artifacts: Vec<serde_json::Value> = response.json();
			assert_eq!(artifacts.len(), 1);
			assert_eq!(artifacts[0]["id"], UNSCOPED, "not group A's");

			// The one it is not offered, by its real id.
			let refused = public
				.get(&format!("/versions/2.60.0/artifacts/{THEIRS}/download"))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;

			// An id that was never registered at all.
			let absent = public
				.get("/versions/2.60.0/artifacts/99999999-9999-9999-9999-999999999999/download")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;

			assert_eq!(
				refused.status_code(),
				absent.status_code(),
				"an artifact held for another group must answer exactly as a missing one does"
			);
			assert_eq!(refused.status_code(), StatusCode::NOT_FOUND);
			assert_eq!(refused.text(), absent.text(), "and say the same thing");
		},
	)
	.await
}

/// Canopy serves the bytes it holds to a caller the artifact is offered to.
#[tokio::test(flavor = "multi_thread")]
async fn the_owning_group_is_served_the_held_bytes() {
	commons_tests::server::run_with_device_auth(
		"machine",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn).await;
			enrol(&mut conn, device_id, GROUP_A).await;

			let response = public
				.get(&format!("/versions/2.60.0/artifacts/{THEIRS}/download"))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;

			response.assert_status_ok();
			assert_eq!(response.text(), "group a schema");
		},
	)
	.await
}

/// Canopy verifies the bytes it holds against the recorded digest as it serves
/// them, so a corrupted artifact fails the read rather than reaching a server
/// as the artifact it is not.
#[tokio::test(flavor = "multi_thread")]
async fn corrupted_bytes_fail_the_read() {
	commons_tests::server::run_with_device_auth(
		"machine",
		async |mut conn, cert, device_id, public, _| {
			seed(&mut conn).await;
			enrol(&mut conn, device_id, GROUP_A).await;

			conn.batch_execute(&format!(
				"UPDATE artifacts SET content = 'tampered'::bytea WHERE id = '{THEIRS}'"
			))
			.await
			.expect("corrupt the stored bytes");

			let response = public
				.get(&format!("/versions/2.60.0/artifacts/{THEIRS}/download"))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.await;

			assert_eq!(response.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
			assert!(!response.text().contains("tampered"));
		},
	)
	.await
}

/// A releaser credential carries no authorisation for any group, so a
/// registration naming one is refused.
#[tokio::test(flavor = "multi_thread")]
async fn a_releaser_cannot_register_for_a_group() {
	commons_tests::server::run_with_device_auth(
		"releaser",
		async |mut conn, cert, _device_id, public, _| {
			seed(&mut conn).await;

			let response = public
				.post(&format!(
					"/artifacts/2.60.0/reporting-schema/any?group={GROUP_A}"
				))
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.text("https://example.com/x.sql")
				.await;

			assert_eq!(response.status_code(), StatusCode::FORBIDDEN);

			// The same registration without a group is accepted.
			let response = public
				.post("/artifacts/2.60.0/installer/windows")
				.add_header("x-forwarded-client-cert", &format!("Cert={cert}"))
				.text("https://example.com/x.exe")
				.await;
			response.assert_status_ok();
		},
	)
	.await
}
