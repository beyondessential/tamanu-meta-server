//! DB-layer tests for the machine grain (`database::machines`).
//!
//! A machine is the host; an application is the software on it. These cover
//! what the model owes at that seam: a machine stands on its own before
//! anything reports, archival travels from box to workload, and an identity
//! resolves to at most one machine.

use commons_tests::db::TestDb;
use database::diesel_async::AsyncPgConnection;
use database::machines::{Machine, NewMachine};
use diesel::{sql_query, sql_types};
use diesel_async::RunQueryDsl;
use uuid::Uuid;

#[derive(diesel::QueryableByName)]
struct RowId {
	#[diesel(sql_type = sql_types::Uuid)]
	id: Uuid,
}

#[derive(diesel::QueryableByName)]
struct Count {
	#[diesel(sql_type = sql_types::BigInt)]
	count: i64,
}

async fn insert_group(conn: &mut AsyncPgConnection, name: &str) -> Uuid {
	sql_query("INSERT INTO server_groups (name) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(name)
		.get_result::<RowId>(conn)
		.await
		.expect("insert group")
		.id
}

async fn insert_device(conn: &mut AsyncPgConnection, role: &str) -> Uuid {
	sql_query("INSERT INTO devices (role) VALUES ($1) RETURNING id")
		.bind::<sql_types::Text, _>(role)
		.get_result::<RowId>(conn)
		.await
		.expect("insert device")
		.id
}

/// An application on a named machine. Applications are still created by the
/// operator path at this point, so the machine is stated rather than reported.
async fn insert_application_on(conn: &mut AsyncPgConnection, machine: Uuid) -> Uuid {
	let host = format!("https://{}.example.invalid", Uuid::new_v4());
	sql_query(
		"INSERT INTO applications (host, kind, machine_id) VALUES ($1, 'central', $2) RETURNING id",
	)
	.bind::<sql_types::Text, _>(host)
	.bind::<sql_types::Uuid, _>(machine)
	.get_result::<RowId>(conn)
	.await
	.expect("insert application")
	.id
}

/// A machine an operator has created but nothing has reported against yet is a
/// legitimate resting state, not an error: it simply has not checked in.
// spec: FLT
#[tokio::test(flavor = "multi_thread")]
async fn a_new_machine_has_no_applications_and_has_not_checked_in() {
	TestDb::run(async |mut conn, _url| {
		let group = insert_group(&mut conn, "g").await;
		let machine = Machine::create(
			&mut conn,
			NewMachine {
				name: Some("box-1".into()),
				group_id: Some(group),
				..Default::default()
			},
		)
		.await
		.expect("create machine");

		assert_eq!(machine.group_id, Some(group));
		assert!(
			machine.registered_at.is_none(),
			"nothing has enrolled against it yet"
		);
		assert!(machine.deleted_at.is_none(), "a new machine is live");
		assert!(
			machine
				.applications(&mut conn)
				.await
				.expect("apps")
				.is_empty(),
			"a machine with nothing on it is fine, not an error"
		);

		let live = Machine::list_live(&mut conn).await.expect("list live");
		assert!(live.iter().any(|m| m.id == machine.id));
	})
	.await;
}

/// A box going away takes its workloads with it. Archival is not deletion:
/// both records remain.
// spec: FLT#archival
#[tokio::test(flavor = "multi_thread")]
async fn archiving_a_machine_archives_the_applications_on_it() {
	TestDb::run(async |mut conn, _url| {
		let machine = Machine::create(&mut conn, NewMachine::default())
			.await
			.expect("create machine");
		let one = insert_application_on(&mut conn, machine.id).await;
		let two = insert_application_on(&mut conn, machine.id).await;

		assert_eq!(
			machine.applications(&mut conn).await.expect("apps").len(),
			2,
			"a machine hosts any number of applications"
		);

		Machine::archive(&mut conn, machine.id)
			.await
			.expect("archive");

		let archived = Machine::get_by_id(&mut conn, machine.id)
			.await
			.expect("machine still readable");
		assert!(archived.deleted_at.is_some(), "the machine is archived");

		let still_there: i64 = sql_query(
			"SELECT count(*) AS count FROM applications \
			 WHERE id IN ($1, $2) AND deleted_at IS NOT NULL",
		)
		.bind::<sql_types::Uuid, _>(one)
		.bind::<sql_types::Uuid, _>(two)
		.get_result::<Count>(&mut conn)
		.await
		.expect("count")
		.count;
		assert_eq!(still_there, 2, "both workloads archived with the box");

		assert!(
			!Machine::list_live(&mut conn)
				.await
				.expect("list live")
				.iter()
				.any(|m| m.id == machine.id),
			"an archived machine leaves the live fleet"
		);
	})
	.await;
}

/// An identity belongs to at most one machine, so resolving one from the other
/// is unambiguous — and an identity that authenticates something else resolves
/// no machine at all.
// spec: FLT#identities
#[tokio::test(flavor = "multi_thread")]
async fn an_identity_resolves_to_at_most_one_machine() {
	TestDb::run(async |mut conn, _url| {
		let device = insert_device(&mut conn, "server").await;
		let machine = Machine::create(&mut conn, NewMachine::default())
			.await
			.expect("create machine");
		sql_query("UPDATE machines SET device_id = $1 WHERE id = $2")
			.bind::<sql_types::Uuid, _>(device)
			.bind::<sql_types::Uuid, _>(machine.id)
			.execute(&mut conn)
			.await
			.expect("attach identity");

		let resolved = Machine::get_by_device_id(&mut conn, device)
			.await
			.expect("resolve")
			.expect("the identity speaks for a machine");
		assert_eq!(resolved.id, machine.id);

		// An admin credential is not a machine's.
		let admin = insert_device(&mut conn, "admin").await;
		assert!(
			Machine::get_by_device_id(&mut conn, admin)
				.await
				.expect("resolve")
				.is_none(),
			"an identity that authenticates something else resolves no machine"
		);

		// The association is exclusive: a second machine cannot take it.
		let other = Machine::create(&mut conn, NewMachine::default())
			.await
			.expect("create machine");
		let taken = sql_query("UPDATE machines SET device_id = $1 WHERE id = $2")
			.bind::<sql_types::Uuid, _>(device)
			.bind::<sql_types::Uuid, _>(other.id)
			.execute(&mut conn)
			.await;
		assert!(taken.is_err(), "an identity belongs to at most one machine");
	})
	.await;
}

/// A machine's group is what the applications on it take, so a group's
/// machines are the unit a deployment is built from.
// spec: FLT#groups
#[tokio::test(flavor = "multi_thread")]
async fn machines_are_listed_by_group_and_exclude_archived() {
	TestDb::run(async |mut conn, _url| {
		let group = insert_group(&mut conn, "deployment").await;
		let other = insert_group(&mut conn, "elsewhere").await;

		let kept = Machine::create(
			&mut conn,
			NewMachine {
				name: Some("kept".into()),
				group_id: Some(group),
				..Default::default()
			},
		)
		.await
		.expect("create");
		let gone = Machine::create(
			&mut conn,
			NewMachine {
				name: Some("gone".into()),
				group_id: Some(group),
				..Default::default()
			},
		)
		.await
		.expect("create");
		Machine::create(
			&mut conn,
			NewMachine {
				name: Some("elsewhere".into()),
				group_id: Some(other),
				..Default::default()
			},
		)
		.await
		.expect("create");

		Machine::archive(&mut conn, gone.id).await.expect("archive");

		let listed = Machine::list_for_group(&mut conn, group)
			.await
			.expect("list for group");
		assert_eq!(
			listed.len(),
			1,
			"archived machines are not in the group's live list"
		);
		assert_eq!(listed[0].id, kept.id);
	})
	.await;
}

/// Every application sits on exactly one machine. The backfill gave each
/// pre-split server a machine of its own, and the legacy operator path keeps
/// that 1:1 until reports create applications against a named machine.
// spec: FLT#cardinality
#[tokio::test(flavor = "multi_thread")]
async fn every_application_has_exactly_one_machine() {
	TestDb::run(async |mut conn, _url| {
		// The legacy path: an application inserted with no machine stated.
		let host = format!("https://{}.example.invalid", Uuid::new_v4());
		let legacy =
			sql_query("INSERT INTO applications (host, kind) VALUES ($1, 'central') RETURNING id")
				.bind::<sql_types::Text, _>(host)
				.get_result::<RowId>(&mut conn)
				.await
				.expect("insert application")
				.id;

		let orphans: i64 =
			sql_query("SELECT count(*) AS count FROM applications WHERE machine_id IS NULL")
				.get_result::<Count>(&mut conn)
				.await
				.expect("count")
				.count;
		assert_eq!(orphans, 0, "an application always runs on a machine");

		// Two applications created that way are on machines of their own,
		// which is the 1:1 the backfill produced.
		let host2 = format!("https://{}.example.invalid", Uuid::new_v4());
		let second =
			sql_query("INSERT INTO applications (host, kind) VALUES ($1, 'central') RETURNING id")
				.bind::<sql_types::Text, _>(host2)
				.get_result::<RowId>(&mut conn)
				.await
				.expect("insert application")
				.id;

		let shared: i64 = sql_query(
			"SELECT count(*) AS count FROM applications a JOIN applications b \
			 ON a.machine_id = b.machine_id AND a.id <> b.id \
			 WHERE a.id IN ($1, $2)",
		)
		.bind::<sql_types::Uuid, _>(legacy)
		.bind::<sql_types::Uuid, _>(second)
		.get_result::<Count>(&mut conn)
		.await
		.expect("count")
		.count;
		assert_eq!(
			shared, 0,
			"the legacy path keeps one application per machine"
		);
	})
	.await;
}
