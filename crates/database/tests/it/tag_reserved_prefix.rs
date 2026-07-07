//! The reserved `canopy:` tag namespace is owned by the synthetic tags the
//! public `/tags` endpoint injects, so operator-driven tag writes on servers
//! and server groups must reject keys under it.

use std::collections::BTreeMap;

use commons_errors::AppError;
use commons_types::server::{TagMap, kind::ServerKind};
use database::{
	pg_duration::PgDuration,
	server_groups::{NewServerGroup, PartialServerGroup, ServerGroup},
	servers::{PartialServer, Server},
	url_field::UrlField,
};
use jiff::SignedDuration;
use uuid::Uuid;

fn reserved_tags() -> TagMap {
	let mut map = BTreeMap::new();
	map.insert("canopy:rank".to_string(), "spoofed".to_string());
	TagMap(map)
}

fn new_server(host: &str) -> Server {
	Server {
		id: Uuid::new_v4(),
		name: Some("t".into()),
		host: Some(UrlField(host.parse().unwrap())),
		kind: ServerKind::Central,
		rank: None,
		device_id: None,
		group_id: None,
		public_name: None,
		cloud: None,
		geolocation: None,
		is_monitored: true,
		allow_legacy_status: false,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
		restore_allowed_until: None,
		restore_allowed_by: None,
	}
}

fn assert_bad_request<T: std::fmt::Debug>(result: Result<T, AppError>) {
	match result {
		Err(AppError::BadRequest(_)) => {}
		other => panic!("expected BadRequest, got {other:?}"),
	}
}

#[tokio::test(flavor = "multi_thread")]
async fn server_create_rejects_reserved_tag_keys() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let mut s = new_server("https://create.example/");
		s.tags = reserved_tags();
		assert_bad_request(Server::create(&mut conn, s).await);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_update_rejects_reserved_tag_keys() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let server = Server::create(&mut conn, new_server("https://update.example/"))
			.await
			.unwrap();
		let updates = PartialServer {
			id: server.id,
			name: None,
			kind: None,
			rank: None,
			host: None,
			device_id: None,
			group_id: None,
			public_name: None,
			cloud: None,
			geolocation: None,
			is_monitored: None,
			allow_legacy_status: None,
			alert_when_down_for: None,
			notes: None,
			tags: Some(reserved_tags()),
		};
		assert_bad_request(Server::update(&mut conn, server.id, updates).await);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_group_create_rejects_reserved_tag_keys() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let new = NewServerGroup {
			name: "reserved-group".into(),
			notes: String::new(),
			tags: reserved_tags(),
			slack_open_delay: None,
		};
		assert_bad_request(ServerGroup::create(&mut conn, new).await);
	})
	.await
}

#[tokio::test(flavor = "multi_thread")]
async fn server_group_update_rejects_reserved_tag_keys() {
	commons_tests::db::TestDb::run(async |mut conn, _url| {
		let group = ServerGroup::create(
			&mut conn,
			NewServerGroup {
				name: "updatable-group".into(),
				notes: String::new(),
				tags: TagMap::default(),
				slack_open_delay: None,
			},
		)
		.await
		.unwrap();
		let changes = PartialServerGroup {
			name: None,
			notes: None,
			tags: Some(reserved_tags()),
			slack_open_delay: None,
		};
		assert_bad_request(ServerGroup::update(&mut conn, group.id, changes).await);
	})
	.await
}
