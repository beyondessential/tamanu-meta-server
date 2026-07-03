use std::time::Duration;

use diesel_async::{
	AsyncPgConnection,
	pooled_connection::{AsyncDieselConnectionManager, mobc::Pool},
};

pub mod admins;
pub mod artifacts;
pub mod backup;
pub mod backups;
pub mod bestool_snippets;
pub mod chrome_releases;
pub mod devices;
pub mod healthcheck_severities;
pub mod issues;
pub mod mcp_tokens;
pub mod notes;
pub mod pg_duration;
pub mod recovery_vault;
pub mod restore;
pub mod schema;
pub mod self_alerts;
pub mod server_enrollment_challenges;
pub mod server_enrollment_tokens;
pub mod server_groups;
pub mod servers;
pub mod silenced_refs;
pub mod slack_outbox;
pub mod sql_playground_history;
pub mod statuses;
pub mod tags;
pub mod tailscale_users;
pub mod url_field;
pub mod version_known_issues;
pub mod versions;
pub mod views;

pub use backups::{
	BackupCredentialIssuance, BackupMaintenanceRun, BackupRecoveryVerification, BackupRepoSnapshot,
	BackupRepoStats, BackupRequest, BackupRun, BackupTypeDefault, NewBackupCredentialIssuance,
	NewBackupRun, NewBackupTypeDefault, NewServerGroupBackupConfig, NewServerGroupBackupSchedule,
	RetentionPolicy, ServerBackupCapability, ServerGroupBackupConfig, ServerGroupBackupSchedule,
};
pub use bestool_snippets::{BestoolSnippet, NewBestoolSnippet};
pub use commons_types::backup::{
	BackupConfigStatus, BackupPurpose, BackupRepoMode, BackupType, MaintenanceKind, RestoreIntent,
	RunOutcome,
};
pub use devices::{Device, DeviceConnection, DeviceKey, DeviceWithInfo};
pub use recovery_vault::RecoveryVaultWrite;
pub use restore::{
	BackupRestoreCheck, NewBackupRestoreCheck, NewRestoreReplica, RestoreConsumerCapability,
	RestoreReplica,
};

pub type Db = Pool<AsyncPgConnection>;

// Re-export for use in other crates
pub use diesel_async;

pub fn init() -> Db {
	init_to(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
}

pub fn init_to(url: &str) -> Db {
	// Bound the pool. Every pod that links this crate (the two servers plus each
	// job) runs its own pool against the same primary; mobc's defaults
	// (max_open=10, idle and lifetimes uncapped) let the fleet's aggregate
	// demand exceed the server's max_connections and pin backends indefinitely.
	// Size per role via env, and recycle connections so a failover doesn't leave
	// the pool holding dead backends.
	let max_open = env_u64("DB_MAX_OPEN_CONNECTIONS", 5);
	let max_idle = env_u64("DB_MAX_IDLE_CONNECTIONS", 2);
	Pool::builder()
		.max_open(max_open)
		.max_idle(max_idle)
		.max_lifetime(Some(Duration::from_secs(30 * 60)))
		.max_idle_lifetime(Some(Duration::from_secs(10 * 60)))
		.build(AsyncDieselConnectionManager::<AsyncPgConnection>::new(url))
}

fn env_u64(key: &str, default: u64) -> u64 {
	std::env::var(key)
		.ok()
		.and_then(|v| v.parse().ok())
		.unwrap_or(default)
}
