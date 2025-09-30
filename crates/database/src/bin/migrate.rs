use clap::Parser;
use database::migrator::UnasyncMigrator;
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

#[tokio::main]
async fn main() -> miette::Result<()> {
	let mut _guard = PreArgs::parse().setup()?;
	let args = Args::parse();
	if _guard.is_none() {
		_guard = Some(args.logging.setup(|v| match v {
			0 => "warn",
			1 => "info",
			2 => "debug",
			_ => "trace",
		})?);
	}

	let mut connection = UnasyncMigrator::connect().await?;
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
