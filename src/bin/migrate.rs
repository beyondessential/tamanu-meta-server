use clap::Parser;
use diesel_migrations::{
	embed_migrations, EmbeddedMigrations, HarnessWithOutput, MigrationHarness as _,
};
use rocket_db_pools::diesel::{AsyncConnection as _, AsyncPgConnection};
pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[derive(Debug, Parser)]
#[command(flatten_help = true)]
struct Cli {
	#[command(subcommand)]
	mode: Option<Mode>,
}

#[derive(Debug, Default, Parser)]
enum Mode {
	/// Run all pending migrations
	#[default]
	Run,

	/// Revert the last migration
	Revert {
		/// The number of migrations to revert
		#[arg(default_value = "1")]
		n: usize,
	},

	/// Redo the last migration
	Redo,

	/// List all migrations
	List,

	/// Exits with 0 if the database is up-to-date, 1 otherwise
	IsOk,
}

#[cfg(feature = "migrations-with-tokio-postgres")]
async fn connection() -> rust_postgres_migrator::UnasyncMigrator {
	let config = tamanu_meta::db_config().unwrap();
	let connection = AsyncPgConnection::establish(&config.url).await.unwrap();
	rust_postgres_migrator::UnasyncMigrator { connection }
}

#[cfg(feature = "migrations-with-libpq")]
async fn connection() -> diesel::pg::PgConnection {
	let config = tamanu_meta::db_config().unwrap();
	diesel::pg::PgConnection::establish(&config.url).unwrap()
}

#[rocket::main]
async fn main() {
	let mut connection = connection().await;
	let mut migrator = HarnessWithOutput::write_to_stdout(&mut connection);

	match Cli::parse().mode.unwrap_or_default() {
		Mode::Run => {
			migrator
				.run_pending_migrations(MIGRATIONS)
				.expect("failed: run migrations");
		}
		Mode::Revert { n } => {
			for _ in 0..n {
				migrator
					.revert_last_migration(MIGRATIONS)
					.expect("failed: revert migration");
			}
		}
		Mode::Redo => {
			migrator
				.revert_last_migration(MIGRATIONS)
				.expect("failed: revert last migration");
			migrator
				.run_pending_migrations(MIGRATIONS)
				.expect("failed: run migrations");
		}
		Mode::List => {
			println!("Pending migrations:");
			for migration in migrator
				.pending_migrations(MIGRATIONS)
				.expect("failed: list migrations")
			{
				println!(
					"{} ({}/up.sql)",
					migration.name().version(),
					migration.name()
				);
			}

			println!("\nApplied migrations:");
			for migration in migrator
				.applied_migrations()
				.expect("failed: list migrations")
			{
				println!("{migration}");
			}
		}
		Mode::IsOk => {
			if migrator
				.has_pending_migration(MIGRATIONS)
				.expect("failed: check if up-to-date")
			{
				std::process::exit(1);
			} else {
				std::process::exit(0);
			}
		}
	}
}

#[cfg(feature = "migrations-with-tokio-postgres")]
mod rust_postgres_migrator {
	//! to avoid needing libpq-dev or building pq-src, we hack up a migration
	//! harness on top of `AsyncPgConnection`, just good enough for migrations.

	use diesel::{
		connection::{BoxableConnection, SimpleConnection},
		pg::Pg,
		sql_query, QueryResult, QueryableByName,
	};
	use diesel_migrations::MigrationHarness;
	use rocket_db_pools::diesel::{
		prelude::RunQueryDsl, AsyncPgConnection, SimpleAsyncConnection as _,
	};

	pub(crate) struct UnasyncMigrator {
		pub(crate) connection: AsyncPgConnection,
	}

	impl SimpleConnection for UnasyncMigrator {
		fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
			let connection = &mut self.connection;
			std::thread::scope(|s| {
				s.spawn(|| {
					let runtime = rocket::tokio::runtime::Builder::new_current_thread()
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

	impl BoxableConnection<Pg> for UnasyncMigrator {
		fn as_any(&self) -> &dyn std::any::Any {
			unimplemented!()
		}

		fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
			unimplemented!()
		}
	}

	impl UnasyncMigrator {
		fn prepare(&mut self) -> diesel::migration::Result<()> {
			self.batch_execute(&format!(
				"CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (
					version varchar(50) primary key not null,
					run_on timestamp without time zone not null default current_timestamp
				)",
			))?;
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
}
