//! Model-level tests for the type axis: what the column stores, what a mixed
//! group's figures resolve to, which applications a version-bearing query
//! covers, and what an agent is told its application is.
//!
//! spec: APP

use commons_types::server::{
	RESERVED_TAG_PREFIX, TagMap, app_type::ApplicationType, rank::ServerRank,
};
use database::{
	applications::Application,
	machines::{Machine, NewMachine},
	pg_duration::PgDuration,
	reported_detail::ReportedDetail,
	server_groups::{NewServerGroup, ServerGroup},
	url_field::UrlField,
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::SignedDuration;
use uuid::Uuid;

fn server(r#type: ApplicationType, rank: Option<ServerRank>, machine_id: Uuid) -> Application {
	Application {
		id: Uuid::new_v4(),
		name: Some(r#type.to_string()),
		host: Some(UrlField(
			format!("https://{type}-{}.example/", Uuid::new_v4())
				.parse()
				.unwrap(),
		)),
		r#type,
		rank,
		device_id: None,
		machine_id,
		group_id: None,
		public_name: None,
		cloud: None,
		geolocation: None,
		is_monitored: true,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
		may_manage_dns: false,
		may_manage_tls: false,
		certificate_profile: None,
		name_management_paused_at: None,
		name_management_paused_by: None,
		name_management_pause_reason: None,
	}
}

/// The box an application runs on, in the same group the application claims.
async fn machine(conn: &mut AsyncPgConnection, group_id: Option<Uuid>) -> Uuid {
	Machine::create(
		conn,
		NewMachine {
			group_id,
			..Default::default()
		},
	)
	.await
	.unwrap()
	.id
}

async fn group(conn: &mut AsyncPgConnection, name: &str) -> ServerGroup {
	ServerGroup::create(
		conn,
		NewServerGroup {
			name: name.into(),
			notes: String::new(),
			tags: TagMap::default(),
			slack_open_delay: None,
			slack_close_delay: None,
		},
	)
	.await
	.unwrap()
}

/// An application's type defaults to a Tamanu central, so a row inserted
/// without one reads as the type the fleet is overwhelmingly made of rather
/// than failing to parse.
#[tokio::test(flavor = "multi_thread")]
async fn type_defaults_to_a_tamanu_central() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		use database::schema::applications;

		let id = Uuid::new_v4();
		diesel::sql_query(
			"WITH m AS (INSERT INTO machines (id) VALUES ($1) RETURNING id) INSERT INTO applications (id, host, machine_id) VALUES ($1, 'https://legacy.example', $1)",
		)
		.bind::<diesel::sql_types::Uuid, _>(id)
		.execute(&mut conn)
		.await
		.unwrap();

		let stored: String = applications::table
			.select(applications::type_)
			.filter(applications::id.eq(id))
			.first(&mut conn)
			.await
			.unwrap();
		assert_eq!(stored, "tamanu-central");
		assert_eq!(
			Application::get_by_id(&mut conn, id).await.unwrap().r#type,
			ApplicationType::TamanuCentral
		);
	})
	.await
}

/// Every type round-trips through the column, so none of them is a value the
/// model can write but not read back.
#[tokio::test(flavor = "multi_thread")]
async fn every_type_round_trips_through_the_column() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		for want in ApplicationType::KNOWN {
			let m = machine(&mut conn, None).await;
			let made = Application::create(&mut conn, server(want.clone(), None, m))
				.await
				.unwrap();
			let loaded = Application::get_by_id(&mut conn, made.id).await.unwrap();
			assert_eq!(
				loaded.r#type, *want,
				"{want} did not survive the round trip"
			);
		}
	})
	.await
}

/// A group holding both a Tamanu server and a SENAITE one takes its headline
/// version from the Tamanu member: the SENAITE server has no version, and
/// letting it speak for the group would blank a figure an operator relies on.
#[tokio::test(flavor = "multi_thread")]
async fn mixed_group_headline_version_comes_from_the_tamanu_member() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let g = group(&mut conn, "pacific").await;

		// The SENAITE server gets the higher rank, so the test bites on which
		// member bears a version rather than on the ordering among equals.
		let lims_machine = machine(&mut conn, Some(g.id)).await;
		let lims = Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(
					ApplicationType::Senaite,
					Some(ServerRank::Production),
					lims_machine,
				)
			},
		)
		.await
		.unwrap();
		let central_machine = machine(&mut conn, Some(g.id)).await;
		let central = Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(
					ApplicationType::TamanuCentral,
					Some(ServerRank::Test),
					central_machine,
				)
			},
		)
		.await
		.unwrap();

		ReportedDetail::record(
			&mut conn,
			central.id,
			central.machine_id,
			"alertd",
			&serde_json::json!({}),
			Some(&"2.34.1".parse().unwrap()),
		)
		.await
		.unwrap();
		ServerGroup::recompute_version(&mut conn, g.id)
			.await
			.unwrap();

		let after = ServerGroup::get_by_id(&mut conn, g.id).await.unwrap();
		assert_eq!(
			after.version_application_id,
			Some(central.id),
			"the versioned member speaks for the group even when outranked"
		);
		assert_eq!(
			after.effective_version.map(|v| v.to_string()),
			Some("2.34.1".to_string())
		);
		let _ = lims;
	})
	.await
}

/// A group of nothing but types canopy holds no release train for has no
/// headline version at all, rather than one borrowed from a member that has
/// none to give.
#[tokio::test(flavor = "multi_thread")]
async fn group_without_a_versioned_member_has_no_headline_version() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let g = group(&mut conn, "labs-only").await;
		let m = machine(&mut conn, Some(g.id)).await;
		Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(ApplicationType::Senaite, Some(ServerRank::Production), m)
			},
		)
		.await
		.unwrap();

		ServerGroup::recompute_version(&mut conn, g.id)
			.await
			.unwrap();

		let after = ServerGroup::get_by_id(&mut conn, g.id).await.unwrap();
		assert_eq!(after.version_application_id, None);
		assert_eq!(after.effective_version, None);
	})
	.await
}

/// A group of facilities still has a headline version. A facility's version is
/// graded against the same release train a central's is, so there is a version
/// to present; a central is only preferred when both are there.
#[tokio::test(flavor = "multi_thread")]
async fn a_group_of_facilities_still_has_a_headline_version() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let g = group(&mut conn, "facilities-only").await;
		let m = machine(&mut conn, Some(g.id)).await;
		let facility = Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(
					ApplicationType::TamanuFacility,
					Some(ServerRank::Production),
					m,
				)
			},
		)
		.await
		.unwrap();

		ReportedDetail::record(
			&mut conn,
			facility.id,
			facility.machine_id,
			"alertd",
			&serde_json::json!({}),
			Some(&"2.31.0".parse().unwrap()),
		)
		.await
		.unwrap();
		ServerGroup::recompute_version(&mut conn, g.id)
			.await
			.unwrap();

		let after = ServerGroup::get_by_id(&mut conn, g.id).await.unwrap();
		assert_eq!(after.version_application_id, Some(facility.id));
		assert_eq!(
			after.effective_version.map(|v| v.to_string()),
			Some("2.31.0".to_string())
		);
	})
	.await
}

/// A central and a facility at one rank is the tie the type ordering exists
/// for: the central speaks for the group.
#[tokio::test(flavor = "multi_thread")]
async fn a_central_outranks_a_facility_on_a_tie() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let g = group(&mut conn, "both").await;
		let fm = machine(&mut conn, Some(g.id)).await;
		let facility = Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(
					ApplicationType::TamanuFacility,
					Some(ServerRank::Production),
					fm,
				)
			},
		)
		.await
		.unwrap();
		let cm = machine(&mut conn, Some(g.id)).await;
		let central = Application::create(
			&mut conn,
			Application {
				group_id: Some(g.id),
				..server(
					ApplicationType::TamanuCentral,
					Some(ServerRank::Production),
					cm,
				)
			},
		)
		.await
		.unwrap();

		for (app, version) in [(&facility, "2.30.0"), (&central, "2.34.1")] {
			ReportedDetail::record(
				&mut conn,
				app.id,
				app.machine_id,
				"alertd",
				&serde_json::json!({}),
				Some(&version.parse().unwrap()),
			)
			.await
			.unwrap();
		}
		ServerGroup::recompute_version(&mut conn, g.id)
			.await
			.unwrap();

		let after = ServerGroup::get_by_id(&mut conn, g.id).await.unwrap();
		assert_eq!(after.version_application_id, Some(central.id));
	})
	.await
}

/// The production-version summary answers what the fleet is running, so it
/// counts only the applications whose type canopy holds a release train for.
#[tokio::test(flavor = "multi_thread")]
async fn production_versions_skip_untracked_types() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let tamanu_machine = machine(&mut conn, None).await;
		let tamanu = Application::create(
			&mut conn,
			server(
				ApplicationType::TamanuCentral,
				Some(ServerRank::Production),
				tamanu_machine,
			),
		)
		.await
		.unwrap();
		let canopy_machine = machine(&mut conn, None).await;
		let canopy = Application::create(
			&mut conn,
			server(
				ApplicationType::Canopy,
				Some(ServerRank::Production),
				canopy_machine,
			),
		)
		.await
		.unwrap();

		ReportedDetail::record(
			&mut conn,
			tamanu.id,
			tamanu.machine_id,
			"alertd",
			&serde_json::json!({}),
			Some(&"2.34.1".parse().unwrap()),
		)
		.await
		.unwrap();
		// A canopy instance does report a version — its own build — which would
		// otherwise land in the fleet's release count as a branch of its own.
		ReportedDetail::record(
			&mut conn,
			canopy.id,
			canopy.machine_id,
			"alertd",
			&serde_json::json!({}),
			Some(&"1.8.0".parse().unwrap()),
		)
		.await
		.unwrap();

		let versions: Vec<String> = ReportedDetail::production_versions(&mut conn)
			.await
			.unwrap()
			.into_iter()
			.map(|v| v.to_string())
			.collect();
		assert_eq!(versions, vec!["2.34.1".to_string()]);
	})
	.await
}

/// The device is told its application's type, and both halves of the pair the
/// type replaced. An agent or an operator rule written against the earlier
/// names keeps matching, so the split does not silently change what a rule
/// selects.
#[tokio::test(flavor = "multi_thread")]
async fn device_tags_carry_the_type_and_both_halves_of_the_pair() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let m = machine(&mut conn, None).await;
		let s = Application::create(&mut conn, server(ApplicationType::Senaite, None, m))
			.await
			.unwrap();

		let tags = s.tags_for_device(&mut conn).await.unwrap();
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}type")),
			Some(&"senaite".to_string())
		);
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}product")),
			Some(&"senaite".to_string())
		);
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}kind")),
			Some(&"standalone".to_string())
		);
	})
	.await
}

/// A Tamanu facility reports the software it is an instance of, not its type,
/// under the earlier name — so the two Tamanu types still bill and match as
/// one product, as they did when product was a field of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_facilitys_tags_still_name_tamanu_as_the_product() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let m = machine(&mut conn, None).await;
		let s = Application::create(&mut conn, server(ApplicationType::TamanuFacility, None, m))
			.await
			.unwrap();

		let tags = s.tags_for_device(&mut conn).await.unwrap();
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}type")),
			Some(&"tamanu-facility".to_string())
		);
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}product")),
			Some(&"tamanu".to_string())
		);
		assert_eq!(
			tags.0.get(&format!("{RESERVED_TAG_PREFIX}kind")),
			Some(&"facility".to_string())
		);
	})
	.await
}

/// The public mobile-app listing covers only a type canopy lists publicly. A
/// SENAITE instance is behind someone else's door and is nobody's to look up,
/// so a public name does not put it on the list.
#[tokio::test(flavor = "multi_thread")]
async fn public_search_excludes_types_that_are_not_listed() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let portal_machine = machine(&mut conn, None).await;
		Application::create(
			&mut conn,
			Application {
				name: Some("Lab Portal".into()),
				public_name: Some("Lab Portal".into()),
				..server(ApplicationType::Senaite, None, portal_machine)
			},
		)
		.await
		.unwrap();
		// A facility is Tamanu too, and still not listable: it sits behind
		// someone else's NAT.
		let facility_machine = machine(&mut conn, None).await;
		Application::create(
			&mut conn,
			Application {
				name: Some("Lab Facility".into()),
				public_name: Some("Lab Facility".into()),
				..server(ApplicationType::TamanuFacility, None, facility_machine)
			},
		)
		.await
		.unwrap();
		let central_machine = machine(&mut conn, None).await;
		Application::create(
			&mut conn,
			Application {
				name: Some("Lab Central".into()),
				public_name: Some("Lab Central".into()),
				..server(ApplicationType::TamanuCentral, None, central_machine)
			},
		)
		.await
		.unwrap();

		let found = Application::search_central(&mut conn, "Lab", 50)
			.await
			.unwrap();
		let names: Vec<String> = found.into_iter().filter_map(|s| s.public_name).collect();
		assert_eq!(names, vec!["Lab Central".to_string()]);
	})
	.await
}

/// A group's shared cost can only be attributed to one product when its
/// members agree on one. They agree on software rather than on type: a central
/// and a facility of one deployment are both Tamanu.
#[tokio::test(flavor = "multi_thread")]
async fn sole_member_software_is_absent_for_a_group_spanning_two() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let tamanu = group(&mut conn, "tamanu-only").await;
		let mixed = group(&mut conn, "mixed").await;

		for (g, r#type) in [
			(tamanu.id, ApplicationType::TamanuCentral),
			// The pair that used to be one product in two roles still is.
			(tamanu.id, ApplicationType::TamanuFacility),
			(mixed.id, ApplicationType::TamanuCentral),
			(mixed.id, ApplicationType::Senaite),
		] {
			let m = machine(&mut conn, Some(g)).await;
			Application::create(
				&mut conn,
				Application {
					group_id: Some(g),
					..server(r#type, None, m)
				},
			)
			.await
			.unwrap();
		}

		let sole = ServerGroup::sole_member_software(&mut conn, &[tamanu.id, mixed.id])
			.await
			.unwrap();
		assert_eq!(
			sole.get(&tamanu.id).map(String::as_str),
			Some("tamanu"),
			"a central and a facility are one software"
		);
		assert_eq!(sole.get(&mixed.id), None, "members span two software");
	})
	.await
}
