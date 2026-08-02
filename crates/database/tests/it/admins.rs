use database::admins::Admin;

/// `Admin::add` is documented as idempotent, and the `/api/admins/add`
/// endpoint declares only success and auth failures — no 404. With
/// `on_conflict_do_nothing` the second add inserted no row, and `get_result`
/// over no rows is `NotFound`, so re-adding an existing admin 404'd.
#[tokio::test(flavor = "multi_thread")]
async fn adding_an_existing_admin_succeeds_and_keeps_the_original_row() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let first = Admin::add(&mut conn, "someone@example.com")
			.await
			.expect("first add");

		let again = Admin::add(&mut conn, "someone@example.com")
			.await
			.expect("re-adding an existing admin must not fail");

		assert_eq!(again.email, first.email);
		assert_eq!(
			again.created_at, first.created_at,
			"the original row is returned, not a fresh one",
		);

		let all = Admin::list(&mut conn).await.expect("list");
		assert_eq!(
			all.iter()
				.filter(|a| a.email == "someone@example.com")
				.count(),
			1,
			"no duplicate row",
		);
	})
	.await
}
