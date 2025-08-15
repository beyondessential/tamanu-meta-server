//! to avoid needing libpq-dev or building pq-src, we hack up a migration
//! harness on top of `AsyncPgConnection`, just good enough for migrations.

use diesel::{
	QueryResult, QueryableByName,
	connection::{BoxableConnection, SimpleConnection},
	deserialize::FromSqlRow,
	migration,
	pg::Pg,
	sql_query,
	sql_types::Untyped,
};
use diesel_async::{
	AsyncConnection as _, AsyncPgConnection, RunQueryDsl as _, SimpleAsyncConnection as _,
};
use diesel_migrations::MigrationHarness;
use miette::IntoDiagnostic;
use tokio::spawn;

pub struct UnasyncMigrator {
	conn: Option<AsyncPgConnection>,
}

impl UnasyncMigrator {
	pub async fn connect() -> miette::Result<Self> {
		Self::connect_to(&std::env::var("DATABASE_URL").unwrap()).await
	}

	pub async fn connect_to(db_url: &str) -> miette::Result<Self> {
		let conn = AsyncPgConnection::establish(db_url)
			.await
			.into_diagnostic()?;
		Ok(UnasyncMigrator::new(conn))
	}

	pub fn new(conn: AsyncPgConnection) -> Self {
		UnasyncMigrator { conn: Some(conn) }
	}

	fn exec(&mut self, query: String) -> QueryResult<()> {
		let (s, r) = std::sync::mpsc::channel();
		let conn = self.conn.take().unwrap();
		spawn(async move {
			let mut conn = conn;
			let result = conn.batch_execute(&query).await;
			s.send((conn, result)).unwrap();
		});
		let (conn, result) = r.recv().unwrap();
		self.conn = Some(conn);
		result
	}

	fn query<T: FromSqlRow<Untyped, Pg> + Send + 'static>(
		&mut self,
		query: String,
	) -> QueryResult<Vec<T>> {
		let (s, r) = std::sync::mpsc::channel();
		let conn = self.conn.take().unwrap();
		spawn(async move {
			let mut conn = conn;
			let result: Result<Vec<T>, _> = sql_query(query).get_results(&mut conn).await;
			s.send((conn, result)).unwrap();
		});
		let (conn, result) = r.recv().unwrap();
		self.conn = Some(conn);
		result
	}
}

impl SimpleConnection for UnasyncMigrator {
	fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
		self.exec(query.into())
	}
}

impl BoxableConnection<Pg> for UnasyncMigrator {
	fn as_any(&self) -> &dyn std::any::Any {
		unimplemented!()
	}

	fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
		unimplemented!()
	}
}

impl UnasyncMigrator {
	fn prepare(&mut self) -> migration::Result<()> {
		self.exec(
			concat!(
				"CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (",
				"version varchar(50) primary key not null, ",
				"run_on timestamp without time zone not null default current_timestamp",
				");",
			)
			.into(),
		)?;
		Ok(())
	}
}

impl MigrationHarness<Pg> for UnasyncMigrator {
	fn run_migration(
		&mut self,
		migration: &dyn migration::Migration<Pg>,
	) -> migration::Result<migration::MigrationVersion<'static>> {
		self.prepare()?;
		migration.run(self)?;
		let version = migration.name().version();
		self.exec(format!(
			"INSERT INTO __diesel_schema_migrations (version) VALUES ('{version}');",
		))?;
		Ok(version.as_owned())
	}

	fn revert_migration(
		&mut self,
		migration: &dyn migration::Migration<Pg>,
	) -> migration::Result<migration::MigrationVersion<'static>> {
		self.prepare()?;
		migration.revert(self)?;
		let version = migration.name().version();
		self.exec(format!(
			"DELETE FROM __diesel_schema_migrations WHERE version = '{version}';",
		))?;
		Ok(version.as_owned())
	}

	fn applied_migrations(
		&mut self,
	) -> migration::Result<Vec<migration::MigrationVersion<'static>>> {
		#[derive(QueryableByName)]
		struct Version {
			#[diesel(sql_type = diesel::sql_types::Text)]
			version: String,
		}

		self.prepare()?;

		let rows: Vec<Version> = self.query(
			"SELECT version FROM __diesel_schema_migrations ORDER BY version DESC;".into(),
		)?;
		Ok(rows.into_iter().map(|v| v.version.into()).collect())
	}
}
