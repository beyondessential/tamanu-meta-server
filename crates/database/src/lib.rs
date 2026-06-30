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
pub mod notes;
pub mod pg_duration;
pub mod restore;
pub mod schema;
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
	Pool::new(AsyncDieselConnectionManager::<AsyncPgConnection>::new(url))
}
