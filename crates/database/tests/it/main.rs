// All integration tests for this crate compile into this single binary.
// One test target links (and writes to disk) once instead of once per
// file, which keeps rebuilds from swamping the machine with I/O.
// Nextest still runs every #[tokio::test] in parallel as usual.

mod backfill_registered_at_migration;
mod backup_detection;
mod backups;
mod check_liveness;
mod check_policies;
mod check_policy_rules;
mod check_severity_map;
mod check_stability;
mod consolidated_checks;
mod event_validation;
mod health_rollup;
mod incident_close_result;
mod incident_get_with_issues;
mod incident_linger;
mod incident_reeval_queue;
mod incident_result_semantics;
mod incident_stats;
mod manual_incidents;
mod mcp_tokens;
mod reachability_sweep;
mod recovery_vault;
mod restore;
mod scoped_check_policies;
mod self_alerts;
mod server_enrollment;
mod server_group_archival;
mod server_group_version_cache;
mod server_restore_window;
mod silenced_health_checks;
mod slack_outbox_enqueue;
mod statuses_device_fk;
mod tag_reserved_prefix;
