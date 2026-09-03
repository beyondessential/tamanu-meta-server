use clap::Parser;
use diesel::Connection;
use diesel_async::{
	AsyncConnection, AsyncPgConnection, async_connection_wrapper::AsyncConnectionWrapper,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness as _, embed_migrations};
use lloggs::{LoggingArgs, PreArgs};
use miette::{IntoDiagnostic, WrapErr, bail, miette};
use std::time::Instant;
use tracing::{error, info};

/// What a migration call hands back, and so what the run's transaction closure
/// must return. Named because it is otherwise too long to write inline.
type MigrationResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

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

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		// `info` at zero verbosity, not `warn`: this binary's whole job is a
		// schema change, and a run that says nothing about which migrations it
		// applied is not observable. A silent success and a silent five-minute
		// stall look identical.
		_guard = Some(args.logging.setup(|v| match v {
			0 => "info",
			1 => "debug",
			_ => "trace",
		})?);
	}

	let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
	let connection = AsyncPgConnection::establish(&database_url)
		.await
		.into_diagnostic()
		.wrap_err("failed to establish database connection")?;

	// The sync wrapper, rather than `AsyncMigrationHarness`, because it is a
	// `diesel::Connection` and so hands us `transaction` — see `Mode::Run`.
	let mut conn: AsyncConnectionWrapper<AsyncPgConnection> =
		AsyncConnectionWrapper::from(connection);

	// The wrapper drives the async connection by blocking on it, which panics on
	// a runtime thread unless the blocking is declared. Every arm below is sync,
	// so one declaration covers the lot.
	tokio::task::block_in_place(|| -> miette::Result<()> {
		match args.mode.unwrap_or_default() {
			Mode::Run => {
				let pending = conn
					.pending_migrations(MIGRATIONS)
					.map_err(|err| miette!("{err}"))
					.wrap_err("failed: list pending migrations")?;

				if pending.is_empty() {
					info!("no pending migrations; database is up to date");
				} else {
					info!("{} migrations pending:", pending.len());
					for migration in &pending {
						info!("  {}", migration.name());
					}

					let run_started = Instant::now();

					// One transaction around the whole run, so a migration that
					// fails part-way through a batch takes its predecessors down
					// with it and leaves the database as it was. Without this, an
					// earlier migration commits, the failing one rolls back on its
					// own, and the database is left half-migrated — no version of
					// the schema anything expects.
					//
					// This holds because no migration here needs to run outside a
					// transaction: Postgres DDL is transactional, and none of them
					// use `CONCURRENTLY`. One that did would have to say so with a
					// `metadata.toml`, and would need lifting out of this.
					//
					// Migrations are driven one at a time rather than by
					// `run_pending_migrations` so that each one can be logged as it
					// starts: a long migration should say what it is before it goes
					// quiet, not after. Diesel's own per-migration transaction
					// nests as a savepoint inside this one.
					//
					// `Connection::` qualified because `AsyncConnection` is also in
					// scope and has its own `transaction`.
					Connection::transaction(&mut conn, |conn| -> MigrationResult {
						for migration in &pending {
							let name = migration.name().to_string();
							info!("applying {name}");

							let started = Instant::now();
							conn.run_migration(migration.as_ref()).inspect_err(|err| {
								error!(
									"{name} failed after {:.1}s: {err}",
									started.elapsed().as_secs_f64()
								);
							})?;

							info!("applied {name} in {:.1}s", started.elapsed().as_secs_f64());
						}

						info!("all {} applied; committing", pending.len());
						Ok(())
					})
					.map_err(|err| miette!("{err}"))
					.wrap_err("failed: run migrations (rolled back; database unchanged)")?;

					info!(
						"committed {} migrations in {:.1}s",
						pending.len(),
						run_started.elapsed().as_secs_f64()
					);
				}
			}
			Mode::Revert { n } => {
				for _ in 0..n {
					let version = conn
						.revert_last_migration(MIGRATIONS)
						.map_err(|err| miette!("{err}"))
						.wrap_err("failed: revert migration")?;
					info!("reverted {version}");
				}
			}
			Mode::Redo => {
				let version = conn
					.revert_last_migration(MIGRATIONS)
					.map_err(|err| miette!("{err}"))
					.wrap_err("failed: revert last migration")?;
				info!("reverted {version}");

				let versions = conn
					.run_pending_migrations(MIGRATIONS)
					.map_err(|err| miette!("{err}"))
					.wrap_err("failed: run migrations")?;
				for version in versions {
					info!("applied {version}");
				}
			}
			Mode::List => {
				println!("Pending migrations:");
				for migration in conn
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
				for migration in conn.applied_migrations().map_err(|err| miette!("{err}"))? {
					println!("{migration}");
				}
			}
			Mode::IsOk => {
				if conn
					.has_pending_migration(MIGRATIONS)
					.map_err(|err| miette!("{err}"))?
				{
					bail!("Pending migrations")
				}
			}
		}

		Ok(())
	})?;

	Ok(())
}
