//! Manual incidents model: create/get roundtrip, list ordering and filters,
//! partial updates (including clearing the end time via `Some(None)`), and
//! delete semantics on known and unknown ids.

use database::manual_incidents::{ManualIncident, ManualIncidentUpdate};
use diesel_async::SimpleAsyncConnection as _;
use jiff::Timestamp;
use uuid::Uuid;

fn ts(s: &str) -> Timestamp {
	s.parse().unwrap()
}

async fn seed_group(conn: &mut database::diesel_async::AsyncPgConnection) -> Uuid {
	let id = Uuid::new_v4();
	conn.batch_execute(&format!(
		"INSERT INTO server_groups (id, name) VALUES ('{id}', 'Manual Group')"
	))
	.await
	.expect("seed group");
	id
}

#[tokio::test(flavor = "multi_thread")]
async fn create_and_get_roundtrip() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let group = seed_group(&mut conn).await;
		let created = ManualIncident::create(
			&mut conn,
			"Fibre cut in Suva",
			"ISP outage took the whole site offline.",
			ts("2026-07-01T10:00:00Z"),
			Some(ts("2026-07-01T12:30:00Z")),
			Some(group),
			"admin@localhost",
		)
		.await
		.expect("create");
		assert_eq!(created.title, "Fibre cut in Suva");
		assert_eq!(
			created.description,
			"ISP outage took the whole site offline."
		);
		assert_eq!(created.started_at, ts("2026-07-01T10:00:00Z"));
		assert_eq!(created.ended_at, Some(ts("2026-07-01T12:30:00Z")));
		assert_eq!(created.server_group_id, Some(group));
		assert_eq!(created.created_by, "admin@localhost");

		let fetched = ManualIncident::get(&mut conn, created.id)
			.await
			.expect("get")
			.expect("present");
		assert_eq!(fetched.id, created.id);
		assert_eq!(fetched.title, created.title);
		assert_eq!(fetched.started_at, created.started_at);
		assert_eq!(fetched.ended_at, created.ended_at);
		assert_eq!(fetched.server_group_id, created.server_group_id);
		assert_eq!(fetched.created_by, created.created_by);

		// Unknown ids: get is None, get_required errors (404).
		assert!(
			ManualIncident::get(&mut conn, Uuid::new_v4())
				.await
				.expect("get")
				.is_none()
		);
		assert!(
			ManualIncident::get_required(&mut conn, Uuid::new_v4())
				.await
				.is_err()
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn list_orders_and_filters() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let group = seed_group(&mut conn).await;
		let ended = ManualIncident::create(
			&mut conn,
			"ended",
			"",
			ts("2026-07-01T10:00:00Z"),
			Some(ts("2026-07-01T11:00:00Z")),
			None,
			"t",
		)
		.await
		.expect("create ended");
		let grouped_ongoing = ManualIncident::create(
			&mut conn,
			"grouped ongoing",
			"",
			ts("2026-07-03T10:00:00Z"),
			None,
			Some(group),
			"t",
		)
		.await
		.expect("create grouped ongoing");
		let ungrouped_ongoing = ManualIncident::create(
			&mut conn,
			"ungrouped ongoing",
			"",
			ts("2026-07-02T10:00:00Z"),
			None,
			None,
			"t",
		)
		.await
		.expect("create ungrouped ongoing");

		let ids = |list: Vec<ManualIncident>| list.into_iter().map(|i| i.id).collect::<Vec<_>>();

		// Most recently started first, regardless of insertion order.
		let all = ManualIncident::list(&mut conn, None, false, 100)
			.await
			.expect("list");
		assert_eq!(
			ids(all),
			vec![grouped_ongoing.id, ungrouped_ongoing.id, ended.id]
		);

		// group filter narrows to that group's incidents.
		let of_group = ManualIncident::list(&mut conn, Some(group), false, 100)
			.await
			.expect("list group");
		assert_eq!(ids(of_group), vec![grouped_ongoing.id]);
		assert!(
			ManualIncident::list(&mut conn, Some(Uuid::new_v4()), false, 100)
				.await
				.expect("list unknown group")
				.is_empty()
		);

		// ongoing_only drops anything with an end time.
		let ongoing = ManualIncident::list(&mut conn, None, true, 100)
			.await
			.expect("list ongoing");
		assert_eq!(ids(ongoing), vec![grouped_ongoing.id, ungrouped_ongoing.id]);

		// Filters combine.
		let grouped_and_ongoing = ManualIncident::list(&mut conn, Some(group), true, 100)
			.await
			.expect("list grouped ongoing");
		assert_eq!(ids(grouped_and_ongoing), vec![grouped_ongoing.id]);

		// limit truncates after ordering: the newest two.
		let limited = ManualIncident::list(&mut conn, None, false, 2)
			.await
			.expect("list limited");
		assert_eq!(ids(limited), vec![grouped_ongoing.id, ungrouped_ongoing.id]);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn update_applies_partial_edits_and_clears_end_time() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let created = ManualIncident::create(
			&mut conn,
			"original title",
			"original description",
			ts("2026-07-01T10:00:00Z"),
			None,
			None,
			"t",
		)
		.await
		.expect("create");

		// Title-only edit leaves every other field alone.
		let updated = ManualIncident::update(
			&mut conn,
			created.id,
			ManualIncidentUpdate {
				title: Some("new title".into()),
				..Default::default()
			},
		)
		.await
		.expect("update")
		.expect("known id");
		assert_eq!(updated.title, "new title");
		assert_eq!(updated.description, "original description");
		assert_eq!(updated.started_at, ts("2026-07-01T10:00:00Z"));
		assert_eq!(updated.ended_at, None);
		assert_eq!(updated.created_by, "t");

		// Setting an end time: Some(Some(_)).
		let updated = ManualIncident::update(
			&mut conn,
			created.id,
			ManualIncidentUpdate {
				ended_at: Some(Some(ts("2026-07-01T14:00:00Z"))),
				..Default::default()
			},
		)
		.await
		.expect("update")
		.expect("known id");
		assert_eq!(updated.ended_at, Some(ts("2026-07-01T14:00:00Z")));
		assert_eq!(updated.title, "new title");

		// Some(None) explicitly clears it, marking the incident ongoing again.
		let updated = ManualIncident::update(
			&mut conn,
			created.id,
			ManualIncidentUpdate {
				ended_at: Some(None),
				..Default::default()
			},
		)
		.await
		.expect("update")
		.expect("known id");
		assert_eq!(updated.ended_at, None);
		assert_eq!(updated.title, "new title");

		// Unknown id → None, not an error.
		assert!(
			ManualIncident::update(&mut conn, Uuid::new_v4(), ManualIncidentUpdate::default())
				.await
				.expect("update unknown")
				.is_none()
		);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_removes_the_record_once() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let created = ManualIncident::create(
			&mut conn,
			"to delete",
			"",
			ts("2026-07-01T10:00:00Z"),
			None,
			None,
			"t",
		)
		.await
		.expect("create");

		assert!(
			ManualIncident::delete(&mut conn, created.id)
				.await
				.expect("delete")
		);
		assert!(
			ManualIncident::get(&mut conn, created.id)
				.await
				.expect("get")
				.is_none()
		);
		// A second delete (and an unknown id) report false.
		assert!(
			!ManualIncident::delete(&mut conn, created.id)
				.await
				.expect("re-delete")
		);
		assert!(
			!ManualIncident::delete(&mut conn, Uuid::new_v4())
				.await
				.expect("delete unknown")
		);
	})
	.await
}
