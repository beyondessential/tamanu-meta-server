//! The in-memory [`BackupSecrets`] store backs onboarding Secret creation +
//! reads in tests and the e2e binary, so cover its round-trip + the
//! missing-secret error path here (the Kube variant needs a live cluster).

use public_server::state::BackupSecrets;

#[tokio::test(flavor = "multi_thread")]
async fn memory_store_round_trips_and_errors_on_missing() {
	let store = BackupSecrets::memory();

	store
		.create_password("backup-repo-x", "password", "correct-horse")
		.await
		.expect("create");
	assert_eq!(
		store
			.read_password("backup-repo-x", "password")
			.await
			.expect("read back"),
		"correct-horse"
	);

	// Missing Secret and missing key both surface as errors (→ 502 in handlers).
	assert!(store.read_password("missing", "password").await.is_err());
	assert!(store.read_password("backup-repo-x", "nope").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn put_keys_sets_exactly_the_given_keyset() {
	use std::collections::BTreeMap;

	let store = BackupSecrets::memory();

	// Dual-key in-flight rotation: both keys present.
	store
		.put_keys(
			"backup-repo-y",
			&BTreeMap::from([
				("password".to_string(), "old".to_string()),
				("password_next".to_string(), "new".to_string()),
			]),
		)
		.await
		.expect("put both");
	let keys = store.read_keys("backup-repo-y").await.expect("read keys");
	assert_eq!(keys.get("password").map(String::as_str), Some("old"));
	assert_eq!(keys.get("password_next").map(String::as_str), Some("new"));

	// Promote: writing only `password` removes the omitted `password_next`.
	store
		.put_keys(
			"backup-repo-y",
			&BTreeMap::from([("password".to_string(), "new".to_string())]),
		)
		.await
		.expect("promote");
	let keys = store.read_keys("backup-repo-y").await.expect("read keys");
	assert_eq!(keys.get("password").map(String::as_str), Some("new"));
	assert!(!keys.contains_key("password_next"), "next cleaned up");

	// read_keys on a missing Secret errors.
	assert!(store.read_keys("missing").await.is_err());
}
