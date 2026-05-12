#[cfg(feature = "ui")]
use std::sync::Arc;

use axum::extract::FromRef;
use commons_errors::Result;
use commons_servers::tailnet_directory::TailnetDirectory;
use database::Db;
#[cfg(feature = "ui")]
use tera::Tera;

#[derive(Clone, Debug)]
pub struct AppState {
	pub db: Db,
	#[cfg(feature = "ui")]
	pub tera: Arc<Tera>,
	#[cfg(feature = "ui")]
	pub server_versions_secret: Option<String>,
	/// Populated only when the public-server's router is nested into
	/// the private-server's `/public/...` mount and the private-server
	/// has a directory configured. The internet-facing public-server
	/// binary's `init()` leaves this `None`, so the tailnet path of
	/// the device-auth extractor can never fire on the open internet.
	pub tailnet_directory: Option<TailnetDirectory>,
}

impl AppState {
	#[cfg(feature = "ui")]
	pub fn init_tera() -> Result<Arc<Tera>> {
		let mut tera = Tera::default();

		macro_rules! embed_template {
			($name:expr) => {
				tera.add_raw_template(
					$name,
					include_str!(concat!("../templates/", $name, ".html.tera")),
				)
				.unwrap();
			};
		}

		embed_template!("artifacts");
		embed_template!("mobile");
		embed_template!("password");
		embed_template!("server_versions");
		embed_template!("versions");

		Ok(Arc::new(tera))
	}

	pub fn init() -> Result<Self> {
		Self::from_db(database::init())
	}

	pub fn from_db(db: Db) -> Result<Self> {
		Self::from_db_with_directory(db, None)
	}

	pub fn from_db_with_directory(
		db: Db,
		tailnet_directory: Option<TailnetDirectory>,
	) -> Result<Self> {
		Ok(Self {
			db,
			#[cfg(feature = "ui")]
			tera: Self::init_tera()?,
			#[cfg(feature = "ui")]
			server_versions_secret: std::env::var("SERVER_VERSIONS_SECRET").ok(),
			tailnet_directory,
		})
	}
}

impl FromRef<AppState> for Db {
	fn from_ref(state: &AppState) -> Self {
		state.db.clone()
	}
}

impl FromRef<AppState> for Option<TailnetDirectory> {
	fn from_ref(state: &AppState) -> Self {
		state.tailnet_directory.clone()
	}
}

#[cfg(feature = "ui")]
impl FromRef<AppState> for Arc<Tera> {
	fn from_ref(state: &AppState) -> Self {
		state.tera.clone()
	}
}
