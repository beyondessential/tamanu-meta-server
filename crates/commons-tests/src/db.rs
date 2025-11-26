use diesel_async::{
	AsyncConnection as _, AsyncMigrationHarness, AsyncPgConnection, SimpleAsyncConnection as _,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};

use miette::{IntoDiagnostic, WrapErr, miette};
use tokio::runtime::{Handle, RuntimeFlavor};
use url::Url;
use uuid::Uuid;

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!();

#[derive(Debug)]
pub struct TestDb {
	name: String,
	url: String,
	admin_url: String,
}

impl TestDb {
	async fn init() -> miette::Result<Self> {
		let _ = tracing_subscriber::fmt::try_init();

		if Handle::current().runtime_flavor() == RuntimeFlavor::CurrentThread {
			panic!(r#"You need to use #[tokio::test(flavor = "multi_thread")]"#);
		}

		let base = Url::parse(
			&std::env::var("DATABASE_URL").expect("DATABASE_URL environment variable is not set"),
		)
		.into_diagnostic()?;
		let mut admin_url = base.clone();
		admin_url.set_path("postgres");

		let name = format!("tamanu_meta_test_{}", Uuid::new_v4().simple());
		tracing::info!("in temporary database {name}");

		let mut url = base.clone();
		url.set_path(&name);

		let this = Self {
			name,
			url: url.to_string(),
			admin_url: admin_url.to_string(),
		};

		this.prepare().await?;

		let conn = this.connect(false).await?;
		let mut migrator = AsyncMigrationHarness::new(conn);
		migrator
			.run_pending_migrations(MIGRATIONS)
			.map_err(|err| miette!("{err}"))
			.wrap_err("failed: run migrations")?;

		Ok(this)
	}

	#[tracing::instrument]
	async fn connect(&self, admin: bool) -> miette::Result<AsyncPgConnection> {
		AsyncPgConnection::establish(if admin { &self.admin_url } else { &self.url })
			.await
			.into_diagnostic()
	}

	async fn prepare(&self) -> miette::Result<()> {
		self.connect(true)
			.await?
			.batch_execute(&format!("CREATE DATABASE \"{}\";", self.name))
			.await
			.into_diagnostic()
	}

	async fn teardown(self) -> miette::Result<()> {
		self.connect(true)
			.await?
			.batch_execute(&format!("DROP DATABASE IF EXISTS \"{}\";", self.name))
			.await
			.into_diagnostic()
	}

	/// Run a test in a temporary database
	pub async fn run<F, T, Fut>(test: F) -> T
	where
		F: FnOnce(AsyncPgConnection, String) -> Fut,
		Fut: Future<Output = T>,
	{
		let tdb = TestDb::init().await.expect("temp db");
		let conn = tdb.connect(false).await.expect("connect to temp db");
		let result = test(conn, tdb.url.clone()).await;
		if let Err(err) = tdb.teardown().await {
			eprintln!("Failed to teardown temp db: {err}");
		}
		result
	}
}
