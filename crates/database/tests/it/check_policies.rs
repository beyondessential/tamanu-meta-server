//! `CheckPolicy` model: catalog of per-(source, check) result policies,
//! maintained by the public-server status ingestion path and edited
//! through the private-server `/api/healthchecks` endpoints.

use commons_types::status::CheckResult;
use database::check_policies::CheckPolicy;

#[tokio::test(flavor = "multi_thread")]
async fn upsert_default_inserts_then_is_idempotent() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::upsert_default(&mut conn, "alertd", "disk_space")
			.await
			.expect("first upsert");
		CheckPolicy::upsert_default(&mut conn, "alertd", "disk_space")
			.await
			.expect("second upsert is no-op");

		// Both upserts should land at the schema default, pending review.
		let rows = CheckPolicy::list(&mut conn).await.expect("list");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].source, "alertd");
		assert_eq!(rows[0].check_name, "disk_space");
		assert_eq!(rows[0].ceiling, CheckResult::Warning);
		assert!(!rows[0].escalates);
		assert!(rows[0].reviewed_at.is_none(), "pending review by default");
		assert!(rows[0].reviewed_by.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn same_check_name_is_distinct_per_source() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::upsert_default(&mut conn, "alertd", "disk_space")
			.await
			.expect("alertd upsert");
		CheckPolicy::upsert_default(&mut conn, "seedling", "disk_space")
			.await
			.expect("seedling upsert");

		CheckPolicy::update(
			&mut conn,
			"alertd",
			"disk_space",
			CheckResult::Failed,
			false,
			None,
			"alice",
		)
		.await
		.expect("update alertd entry");

		let seedling = CheckPolicy::get(&mut conn, "seedling", "disk_space")
			.await
			.expect("get")
			.expect("seedling row exists");
		assert_eq!(
			seedling.ceiling,
			CheckResult::Warning,
			"the other source's entry is untouched",
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_default_does_not_overwrite_operator_policy() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::upsert_default(&mut conn, "alertd", "disk_space")
			.await
			.expect("seed");
		CheckPolicy::update(
			&mut conn,
			"alertd",
			"disk_space",
			CheckResult::Failed,
			true,
			None,
			"alice",
		)
		.await
		.expect("operator update");

		// A subsequent push from ingestion must not revert the row.
		CheckPolicy::upsert_default(&mut conn, "alertd", "disk_space")
			.await
			.expect("upsert again");

		let rows = CheckPolicy::list(&mut conn).await.expect("list");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].ceiling, CheckResult::Failed);
		assert!(rows[0].escalates);
		assert!(rows[0].reviewed_at.is_some());
		assert_eq!(rows[0].reviewed_by.as_deref(), Some("alice"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn apply_caps_at_ceiling_or_defaults_to_warning() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let empty_map = serde_json::Map::new();
		let empty_tags = std::collections::HashMap::new();
		let ctx = database::check_policies::EvaluationContext {
			status_extra: &empty_map,
			check_extra: &empty_map,
			tags: &empty_tags,
		};
		// Unknown check → default policy (programmer-error path;
		// ingestion upserts before reading in production).
		let unknown = CheckPolicy::apply(&mut conn, "alertd", "ghost", CheckResult::Failed, &ctx)
			.await
			.expect("apply");
		assert_eq!(unknown.effective, CheckResult::Warning);
		assert!(!unknown.escalates);

		CheckPolicy::upsert_default(&mut conn, "alertd", "cert_expiry")
			.await
			.expect("seed");
		CheckPolicy::update(
			&mut conn,
			"alertd",
			"cert_expiry",
			CheckResult::Failed,
			true,
			None,
			"bob",
		)
		.await
		.expect("update");

		// A failed ceiling passes failures through, carrying the flag.
		let failed = CheckPolicy::apply(
			&mut conn,
			"alertd",
			"cert_expiry",
			CheckResult::Failed,
			&ctx,
		)
		.await
		.expect("apply");
		assert_eq!(failed.effective, CheckResult::Failed);
		assert!(failed.escalates);

		// A warning observation is already below the ceiling: unchanged.
		let warned = CheckPolicy::apply(
			&mut conn,
			"alertd",
			"cert_expiry",
			CheckResult::Warning,
			&ctx,
		)
		.await
		.expect("apply");
		assert_eq!(warned.effective, CheckResult::Warning);

		// A passed ceiling means the check never alerts.
		CheckPolicy::update(
			&mut conn,
			"alertd",
			"cert_expiry",
			CheckResult::Passed,
			false,
			None,
			"bob",
		)
		.await
		.expect("downgrade");
		let ignored = CheckPolicy::apply(
			&mut conn,
			"alertd",
			"cert_expiry",
			CheckResult::Failed,
			&ctx,
		)
		.await
		.expect("apply");
		assert_eq!(ignored.effective, CheckResult::Passed);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_stamps_review_metadata_even_on_no_op_save() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		CheckPolicy::upsert_default(&mut conn, "alertd", "noisy_check")
			.await
			.expect("seed");

		// "Mark reviewed without changing the policy": pass the current
		// (default) values. reviewed_at + reviewed_by must still be set.
		let updated = CheckPolicy::update(
			&mut conn,
			"alertd",
			"noisy_check",
			CheckResult::Warning,
			false,
			None,
			"carol",
		)
		.await
		.expect("update");
		assert_eq!(updated.ceiling, CheckResult::Warning);
		assert!(updated.reviewed_at.is_some());
		assert_eq!(updated.reviewed_by.as_deref(), Some("carol"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_orders_by_source_then_check_name() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for (source, name) in [("alertd", "zeta"), ("alertd", "alpha"), ("seedling", "mu")] {
			CheckPolicy::upsert_default(&mut conn, source, name)
				.await
				.expect("seed");
		}
		let rows = CheckPolicy::list(&mut conn).await.expect("list");
		let keys: Vec<(&str, &str)> = rows
			.iter()
			.map(|r| (r.source.as_str(), r.check_name.as_str()))
			.collect();
		assert_eq!(
			keys,
			vec![("alertd", "alpha"), ("alertd", "zeta"), ("seedling", "mu"),]
		);
	})
	.await
}
