//! The in-memory [`BackupSecrets`] store backs onboarding Secret creation +
//! escrow reveal in tests and the e2e binary, so cover its round-trip + the
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
