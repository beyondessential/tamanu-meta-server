//! Model-level tests for the product axis: what the migration backfilled, what
//! a mixed-product group's figures resolve to, and which servers a
//! version-bearing query covers.
//!
//! spec: APP

use commons_types::server::{
	RESERVED_TAG_PREFIX, TagMap, kind::ServerKind, product::Product, rank::ServerRank,
};
use database::{
	pg_duration::PgDuration,
	reported_detail::ReportedDetail,
	server_groups::{NewServerGroup, ServerGroup},
	servers::Server,
	url_field::UrlField,
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::SignedDuration;
use uuid::Uuid;

fn server(product: Product, kind: ServerKind, rank: Option<ServerRank>) -> Server {
	Server {
		id: Uuid::new_v4(),
		name: Some(format!("{product}-{kind}")),
		host: Some(UrlField(
			format!("https://{product}-{}.example/", Uuid::new_v4())
				.parse()
				.unwrap(),
		)),
		product,
		kind,
		rank,
		device_id: None,
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
		restore_allowed_until: None,
		restore_allowed_by: None,
	}
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

/// A server's product defaults to Tamanu, so every row that predates the
/// column reads as one.
#[tokio::test(flavor = "multi_thread")]
async fn product_defaults_to_tamanu() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		use database::schema::servers;

		let id = Uuid::new_v4();
		diesel::sql_query(
			"INSERT INTO servers (id, host, kind) VALUES ($1, 'https://legacy.example', 'central')",
		)
		.bind::<diesel::sql_types::Uuid, _>(id)
		.execute(&mut conn)
		.await
		.unwrap();

		let stored: String = servers::table
			.select(servers::product)
			.filter(servers::id.eq(id))
			.first(&mut conn)
			.await
			.unwrap();
		assert_eq!(stored, "tamanu");
		assert_eq!(
			Server::get_by_id(&mut conn, id).await.unwrap().product,
			Product::Tamanu
		);
	})
	.await
}

/// The migration lifted canopy instances onto `product` and deliberately left
/// `kind` alone, so a row still carrying the old kind value has to read as
/// standalone rather than fail to parse.
#[tokio::test(flavor = "multi_thread")]
async fn legacy_canopy_kind_still_reads() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let id = Uuid::new_v4();
		diesel::sql_query(
			"INSERT INTO servers (id, host, kind, product) \
			 VALUES ($1, 'https://canopy.example', 'canopy', 'canopy')",
		)
		.bind::<diesel::sql_types::Uuid, _>(id)
		.execute(&mut conn)
		.await
		.unwrap();

		let loaded = Server::get_by_id(&mut conn, id).await.unwrap();
		assert_eq!(loaded.product, Product::Canopy);
		assert_eq!(loaded.kind, ServerKind::Standalone);
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

		// The SENAITE server outranks nothing, but it sorts first by kind
		// priority among equals if product isn't considered — standalone would
		// still lose to central, so give it the higher rank to make the test
		// bite on product rather than on the ordering.
		let lims = Server::create(
			&mut conn,
			Server {
				group_id: Some(g.id),
				..server(
					Product::Senaite,
					ServerKind::Standalone,
					Some(ServerRank::Production),
				)
			},
		)
		.await
		.unwrap();
		let central = Server::create(
			&mut conn,
			Server {
				group_id: Some(g.id),
				..server(Product::Tamanu, ServerKind::Central, Some(ServerRank::Test))
			},
		)
		.await
		.unwrap();

		ReportedDetail::record(
			&mut conn,
			central.id,
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
			after.version_server_id,
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

/// A group of nothing but products canopy holds no release train for has no
/// headline version at all, rather than one borrowed from a member that has
/// none to give.
#[tokio::test(flavor = "multi_thread")]
async fn group_without_a_versioned_member_has_no_headline_version() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let g = group(&mut conn, "labs-only").await;
		Server::create(
			&mut conn,
			Server {
				group_id: Some(g.id),
				..server(
					Product::Senaite,
					ServerKind::Standalone,
					Some(ServerRank::Production),
				)
			},
		)
		.await
		.unwrap();

		ServerGroup::recompute_version(&mut conn, g.id)
			.await
			.unwrap();

		let after = ServerGroup::get_by_id(&mut conn, g.id).await.unwrap();
		assert_eq!(after.version_server_id, None);
		assert_eq!(after.effective_version, None);
	})
	.await
}

/// The production-version summary answers what the fleet is running, so it
/// counts only the servers whose product canopy holds a release train for.
#[tokio::test(flavor = "multi_thread")]
async fn production_versions_skip_untracked_products() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let tamanu = Server::create(
			&mut conn,
			server(
				Product::Tamanu,
				ServerKind::Central,
				Some(ServerRank::Production),
			),
		)
		.await
		.unwrap();
		let canopy = Server::create(
			&mut conn,
			server(
				Product::Canopy,
				ServerKind::Standalone,
				Some(ServerRank::Production),
			),
		)
		.await
		.unwrap();

		ReportedDetail::record(
			&mut conn,
			tamanu.id,
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

/// The device gets its server's product alongside its kind, so an agent can
/// read the classification canopy holds for it.
#[tokio::test(flavor = "multi_thread")]
async fn device_tags_carry_the_product() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let s = Server::create(
			&mut conn,
			server(Product::Senaite, ServerKind::Standalone, None),
		)
		.await
		.unwrap();

		let tags = s.tags_for_device(&mut conn).await.unwrap();
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

/// The public mobile-app listing covers only a product canopy lists publicly,
/// stated rather than left to fall out of the kind filter.
#[tokio::test(flavor = "multi_thread")]
async fn public_search_excludes_products_that_are_not_listed() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		// Deliberately give the SENAITE server the central kind and a public
		// name, so only the product filter can keep it out.
		Server::create(
			&mut conn,
			Server {
				name: Some("Lab Portal".into()),
				public_name: Some("Lab Portal".into()),
				..server(Product::Senaite, ServerKind::Central, None)
			},
		)
		.await
		.unwrap();
		Server::create(
			&mut conn,
			Server {
				name: Some("Lab Central".into()),
				public_name: Some("Lab Central".into()),
				..server(Product::Tamanu, ServerKind::Central, None)
			},
		)
		.await
		.unwrap();

		let found = Server::search_central(&mut conn, "Lab", 50).await.unwrap();
		let names: Vec<String> = found.into_iter().filter_map(|s| s.public_name).collect();
		assert_eq!(names, vec!["Lab Central".to_string()]);
	})
	.await
}

/// A group's shared cost can only be attributed to one product when its
/// members agree on one.
#[tokio::test(flavor = "multi_thread")]
async fn sole_member_product_is_absent_for_a_mixed_group() {
	commons_tests::db::TestDb::run(|mut conn, _url| async move {
		let pure = group(&mut conn, "tamanu-only").await;
		let mixed = group(&mut conn, "mixed").await;

		for (g, product) in [
			(pure.id, Product::Tamanu),
			(mixed.id, Product::Tamanu),
			(mixed.id, Product::Senaite),
		] {
			let kind = product.default_kind();
			Server::create(
				&mut conn,
				Server {
					group_id: Some(g),
					..server(product, kind, None)
				},
			)
			.await
			.unwrap();
		}

		let sole = ServerGroup::sole_member_products(&mut conn, &[pure.id, mixed.id])
			.await
			.unwrap();
		assert_eq!(sole.get(&pure.id), Some(&Product::Tamanu));
		assert_eq!(sole.get(&mixed.id), None, "members span products");
	})
	.await
}
