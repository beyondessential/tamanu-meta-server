use std::fmt;

use diesel::{QueryableByName, connection::SimpleConnection, pg::Pg, sql_query};
use diesel_migrations::{
	EmbeddedMigrations, HarnessWithOutput, MigrationHarness, embed_migrations,
};
use rocket_db_pools::diesel::{
	AsyncConnection as _, AsyncPgConnection, SimpleAsyncConnection as _, prelude::RunQueryDsl,
};
use url::Url;
use uuid::Uuid;

/// Embedded diesel migrations from the project's `migrations/` directory.
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

/// A simple error type for test DB helpers.
pub type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Encapsulates a temporary test database lifecycle:
/// - create a uniquely-named database
/// - run migrations on it
/// - provide helpers to connect
/// - drop it when you're done via `teardown()`
///
/// NOTE:
/// - You must ensure all connections to the temp DB are dropped before calling `teardown()`.
/// - This helper assumes the DB user has permissions to CREATE/DROP DATABASE.
pub struct TestDb {
	/// The unique database name created for this test run.
	pub name: String,
	/// The full connection URL to the temporary test database.
	pub url: String,
	/// The admin connection URL pointing to the maintenance database (e.g., `postgres`).
	admin_url: String,
}

impl fmt::Debug for TestDb {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("TestDb")
			.field("name", &self.name)
			.field("url", &self.url)
			.finish_non_exhaustive()
	}
}

impl TestDb {
	/// Creates a temporary database using the application's configured DB URL,
	/// runs embedded migrations on it, and returns a handle to manage it.
	async fn from_config() -> TestResult<Self> {
		let config = tamanu_meta::db_config()?;
		Self::from_base_url(&config.url).await
	}

	/// Creates a temporary database using the provided base URL,
	/// runs embedded migrations on it, and returns a handle to manage it.
	///
	/// The base URL should be a standard Postgres URL, e.g.:
	/// - postgres://user:pass@host:port/dbname
	/// The `dbname` in the base URL is used only to derive the authority and is not used directly.
	async fn from_base_url(base_url: &str) -> TestResult<Self> {
		let base = Url::parse(base_url)?;
		let mut admin_url = base.clone();
		// Use the maintenance DB for admin operations
		admin_url.set_path("postgres");

		let db_name = format!("tamanu_meta_test_{}", Uuid::new_v4().simple());
		eprintln!("Using new temporary database {db_name}");

		let mut test_url = base.clone();
		test_url.set_path(&db_name);

		// Create temp database via admin connection
		let mut admin = AsyncPgConnection::establish(admin_url.as_str()).await?;
		admin
			.batch_execute(&format!("CREATE DATABASE \"{}\";", db_name))
			.await?;

		// Run embedded migrations against the new database
		run_migrations(test_url.as_str()).await?;

		Ok(Self {
			name: db_name,
			url: test_url.to_string(),
			admin_url: admin_url.to_string(),
		})
	}

	/// Establish a new connection to the temporary database.
	async fn connect(&self) -> TestResult<AsyncPgConnection> {
		let conn = AsyncPgConnection::establish(&self.url).await?;
		Ok(conn)
	}

	/// Drops the temporary database.
	///
	/// IMPORTANT: Ensure all connections to `self.url` are dropped before calling this,
	/// otherwise Postgres will refuse to drop the database.
	async fn teardown(self) -> TestResult<()> {
		let mut admin = AsyncPgConnection::establish(&self.admin_url).await?;
		admin
			.batch_execute(&format!("DROP DATABASE IF EXISTS \"{}\";", self.name))
			.await?;
		Ok(())
	}

	/// Run a test in a temporary database
	pub async fn run<F, T, Fut>(test: F) -> T
	where
		F: FnOnce(AsyncPgConnection) -> Fut,
		Fut: Future<Output = T>,
	{
		let tdb = TestDb::from_config().await.expect("temp db");
		let conn = tdb.connect().await.expect("connect to temp db");
		let result = test(conn).await;
		if let Err(err) = tdb.teardown().await {
			eprintln!("Failed to teardown temp db: {err}");
		}
		result
	}
}

/// Run embedded diesel migrations on the given database URL using an async connection.
///
/// This uses a small compatibility harness to drive diesel_migrations on top of
/// an AsyncPgConnection without requiring libpq.
pub async fn run_migrations(db_url: &str) -> TestResult<()> {
	let conn = AsyncPgConnection::establish(db_url).await?;

	let mut migrator = UnasyncMigrator { connection: conn };
	let mut harness = HarnessWithOutput::write_to_stdout(&mut migrator);
	harness.run_pending_migrations(MIGRATIONS)?;
	Ok(())
}

/// A minimal migration harness adapted to work with `AsyncPgConnection` by delegating
/// to a current-thread Tokio runtime to execute async DB operations while satisfying
/// diesel_migrations' synchronous `MigrationHarness` trait.
struct UnasyncMigrator {
	connection: AsyncPgConnection,
}

impl diesel::connection::SimpleConnection for UnasyncMigrator {
	fn batch_execute(&mut self, query: &str) -> diesel::QueryResult<()> {
		let connection = &mut self.connection;
		std::thread::scope(|s| {
			s.spawn(|| {
				let runtime = rocket::tokio::runtime::Builder::new_current_thread()
					.enable_all()
					.build()
					.unwrap();
				runtime.block_on(async move {
					connection.batch_execute(query).await?;
					Ok(())
				})
			})
			.join()
		})
		.unwrap()
	}
}

impl diesel::connection::BoxableConnection<Pg> for UnasyncMigrator {
	fn as_any(&self) -> &dyn std::any::Any {
		unimplemented!()
	}

	fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
		unimplemented!()
	}
}

impl UnasyncMigrator {
	fn prepare(&mut self) -> diesel::migration::Result<()> {
		self.batch_execute(
			"CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (
				version varchar(50) primary key not null,
				run_on timestamp without time zone not null default current_timestamp
			)",
		)?;
		Ok(())
	}
}

impl MigrationHarness<Pg> for UnasyncMigrator {
	fn run_migration(
		&mut self,
		migration: &dyn diesel::migration::Migration<Pg>,
	) -> diesel::migration::Result<diesel::migration::MigrationVersion<'static>> {
		self.prepare()?;
		migration.run(self)?;
		let version = migration.name().version();
		self.batch_execute(&format!(
			"INSERT INTO __diesel_schema_migrations (version) VALUES ('{version}')",
		))?;
		Ok(version.as_owned())
	}

	fn revert_migration(
		&mut self,
		migration: &dyn diesel::migration::Migration<Pg>,
	) -> diesel::migration::Result<diesel::migration::MigrationVersion<'static>> {
		self.prepare()?;
		migration.revert(self)?;
		let version = migration.name().version();
		self.batch_execute(&format!(
			"DELETE FROM __diesel_schema_migrations WHERE version = '{version}'",
		))?;
		Ok(version.as_owned())
	}

	fn applied_migrations(
		&mut self,
	) -> diesel::migration::Result<Vec<diesel::migration::MigrationVersion<'static>>> {
		#[derive(QueryableByName)]
		struct Version {
			#[diesel(sql_type = diesel::sql_types::Text)]
			version: String,
		}

		self.prepare()?;

		std::thread::scope(|s| {
			s.spawn(|| {
				let runtime = rocket::tokio::runtime::Builder::new_current_thread()
					.enable_all()
					.build()
					.unwrap();
				runtime.block_on(async move {
					let rows: Vec<Version> = sql_query(
						"SELECT version FROM __diesel_schema_migrations ORDER BY version DESC",
					)
					.get_results(&mut self.connection)
					.await?;
					Ok(rows.into_iter().map(|v| v.version.into()).collect())
				})
			})
			.join()
		})
		.unwrap()
	}
}
