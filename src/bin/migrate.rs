use clap::Parser;
use diesel::{pg::PgConnection, Connection};
use diesel_migrations::{
	embed_migrations, EmbeddedMigrations, HarnessWithOutput, MigrationHarness,
};
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
		steps: usize,
	},

	/// Redo the last migration
	Redo,

	/// List all migrations
	List,

	/// Exits with 0 if the database is up-to-date, 1 otherwise
	IsOk,
}

#[rocket::main]
async fn main() {
	let mode = Cli::parse().mode.unwrap_or_default();
	let config = tamanu_meta::db_config().unwrap();
	let mut connection = PgConnection::establish(&config.url).unwrap();
	let mut migrator = HarnessWithOutput::write_to_stdout(&mut connection);

	match mode {
		Mode::Run => {
			migrator
				.run_pending_migrations(MIGRATIONS)
				.expect("failed: run migrations");
		}
		Mode::Revert { steps } => {
			for _ in 0..steps {
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
