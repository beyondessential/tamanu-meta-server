//! `HealthcheckSeverity` model: catalog of healthcheck names → severity,
//! maintained by the public-server status ingestion path and edited
//! through the private-server `/api/healthchecks` endpoints.

use commons_types::issue::Severity;
use database::healthcheck_severities::HealthcheckSeverity;

#[tokio::test(flavor = "multi_thread")]
async fn upsert_default_inserts_then_is_idempotent() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "disk_space")
			.await
			.expect("first upsert");
		HealthcheckSeverity::upsert_default(&mut conn, "disk_space")
			.await
			.expect("second upsert is no-op");

		// Both upserts should land at the schema default, pending review.
		let rows = HealthcheckSeverity::list(&mut conn).await.expect("list");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].check_name, "disk_space");
		assert_eq!(rows[0].severity, Severity::Warning);
		assert!(rows[0].reviewed_at.is_none(), "pending review by default");
		assert!(rows[0].reviewed_by.is_none());
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn upsert_default_does_not_overwrite_operator_severity() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "disk_space")
			.await
			.expect("seed");
		HealthcheckSeverity::update(&mut conn, "disk_space", Severity::Error, None, "alice")
			.await
			.expect("operator update");

		// A subsequent push from ingestion must not revert the row.
		HealthcheckSeverity::upsert_default(&mut conn, "disk_space")
			.await
			.expect("upsert again");

		let rows = HealthcheckSeverity::list(&mut conn).await.expect("list");
		assert_eq!(rows.len(), 1);
		assert_eq!(rows[0].severity, Severity::Error);
		assert!(rows[0].reviewed_at.is_some());
		assert_eq!(rows[0].reviewed_by.as_deref(), Some("alice"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn severity_for_returns_catalog_value_or_warning_default() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		let empty_map = serde_json::Map::new();
		let empty_tags = std::collections::HashMap::new();
		let ctx = database::healthcheck_severities::EvaluationContext {
			status_extra: &empty_map,
			check_extra: &empty_map,
			tags: &empty_tags,
		};
		// Unknown check → fallback (programmer-error path; ingestion
		// upserts before reading in production).
		let unknown = HealthcheckSeverity::severity_for(&mut conn, "ghost", &ctx)
			.await
			.expect("lookup");
		assert_eq!(unknown, Severity::Warning);

		HealthcheckSeverity::upsert_default(&mut conn, "cert_expiry")
			.await
			.expect("seed");
		HealthcheckSeverity::update(&mut conn, "cert_expiry", Severity::Critical, None, "bob")
			.await
			.expect("update");

		let known = HealthcheckSeverity::severity_for(&mut conn, "cert_expiry", &ctx)
			.await
			.expect("lookup");
		assert_eq!(known, Severity::Critical);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_stamps_review_metadata_even_on_no_op_save() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		HealthcheckSeverity::upsert_default(&mut conn, "noisy_check")
			.await
			.expect("seed");

		// "Mark reviewed without changing severity": pass the current
		// (default) value. reviewed_at + reviewed_by must still be set.
		let updated =
			HealthcheckSeverity::update(&mut conn, "noisy_check", Severity::Warning, None, "carol")
				.await
				.expect("update");
		assert_eq!(updated.severity, Severity::Warning);
		assert!(updated.reviewed_at.is_some());
		assert_eq!(updated.reviewed_by.as_deref(), Some("carol"));
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_orders_by_check_name() {
	commons_tests::db::TestDb::run(async |mut conn, _| {
		for name in ["zeta", "alpha", "mu"] {
			HealthcheckSeverity::upsert_default(&mut conn, name)
				.await
				.expect("seed");
		}
		let rows = HealthcheckSeverity::list(&mut conn).await.expect("list");
		let names: Vec<&str> = rows.iter().map(|r| r.check_name.as_str()).collect();
		assert_eq!(names, vec!["alpha", "mu", "zeta"]);
	})
	.await
}
