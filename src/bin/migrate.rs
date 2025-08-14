use clap::Parser;
use diesel_migrations::{
	EmbeddedMigrations, HarnessWithOutput, MigrationHarness as _, embed_migrations,
};
use lloggs::{LoggingArgs, PreArgs};
use miette::{WrapErr, bail, miette};

#[derive(Debug, Parser)]
#[command(flatten_help = true)]
struct Args {
	#[command(flatten)]
	logging: LoggingArgs,

	#[command(subcommand)]
	mode: Option<Mode>,
}

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

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

async fn connection() -> miette::Result<rust_postgres_migrator::UnasyncMigrator> {
	let pool = tamanu_meta::state::AppState::init_db()?;
	Ok(rust_postgres_migrator::UnasyncMigrator { pool })
}

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}

	let mut connection = connection().await?;
	let mut migrator = HarnessWithOutput::write_to_stdout(&mut connection);

	match args.mode.unwrap_or_default() {
		Mode::Run => {
			migrator
				.run_pending_migrations(MIGRATIONS)
				.map_err(|err| miette!("{err}"))
				.wrap_err("failed: run migrations")?;
		}
		Mode::Revert { n } => {
			for _ in 0..n {
				migrator
					.revert_last_migration(MIGRATIONS)
					.map_err(|err| miette!("{err}"))
					.wrap_err("failed: revert migration")?;
			}
		}
		Mode::Redo => {
			migrator
				.revert_last_migration(MIGRATIONS)
				.map_err(|err| miette!("{err}"))
				.wrap_err("failed: revert last migration")?;
			migrator
				.run_pending_migrations(MIGRATIONS)
				.map_err(|err| miette!("{err}"))
				.wrap_err("failed: run migrations")?;
		}
		Mode::List => {
			println!("Pending migrations:");
			for migration in migrator
				.pending_migrations(MIGRATIONS)
				.map_err(|err| miette!("{err}"))?
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
				.map_err(|err| miette!("{err}"))?
			{
				println!("{migration}");
			}
		}
		Mode::IsOk => {
			if migrator
				.has_pending_migration(MIGRATIONS)
				.map_err(|err| miette!("{err}"))?
			{
				bail!("Pending migrations")
			}
		}
	}

	Ok(())
}

mod rust_postgres_migrator {
	//! to avoid needing libpq-dev or building pq-src, we hack up a migration
	//! harness on top of `AsyncPgConnection`, just good enough for migrations.

	use diesel::{
		QueryResult, QueryableByName,
		connection::{BoxableConnection, SimpleConnection},
		pg::Pg,
		sql_query,
	};
	use diesel_async::SimpleAsyncConnection as _;
	use diesel_migrations::MigrationHarness;
	use tamanu_meta::state::Db;

	pub(crate) struct UnasyncMigrator {
		pub(crate) pool: Db,
	}

	impl SimpleConnection for UnasyncMigrator {
		fn batch_execute(&mut self, query: &str) -> QueryResult<()> {
			std::thread::scope(|s| {
				s.spawn(|| {
					let runtime = tokio::runtime::Builder::new_current_thread()
						.build()
						.unwrap();
					runtime.block_on(async move {
						let mut connection = self.pool.get().await.unwrap();
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
						.build()
						.unwrap();
					runtime.block_on(async move {
						use diesel_async::RunQueryDsl as _;
						let mut connection = self.pool.get().await.unwrap();
						let rows: Vec<Version> = sql_query(
							"SELECT version FROM __diesel_schema_migrations ORDER BY version DESC",
						)
						.get_results(&mut connection)
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
