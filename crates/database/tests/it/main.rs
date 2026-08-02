// All integration tests for this crate compile into this single binary.
// One test target links (and writes to disk) once instead of once per
// file, which keeps rebuilds from swamping the machine with I/O.
// Nextest still runs every #[tokio::test] in parallel as usual.

mod admins;
mod backfill_registered_at_migration;
mod backup_detection;
mod backups;
mod certificate_alerts;
mod check_liveness;
mod check_policies;
mod check_policy_rules;
mod check_severity_map;
mod check_stability;
mod chrome_releases;
mod consolidated_checks;
mod event_validation;
mod fleet_check_detail;
mod health_rollup;
mod incident_close_result;
mod incident_get_with_issues;
mod incident_linger;
mod incident_list_status;
mod incident_open_race;
mod incident_reeval_queue;
mod incident_result_semantics;
mod incident_stats;
mod issue_list_filters;
mod mcp_tokens;
mod migration_test_candidates;
mod migration_test_reports;
mod partitions;
mod reachability_sweep;
mod recovery_vault;
mod reported_detail;
mod restore;
mod rotation_interlock;
mod scope;
mod scoped_check_policies;
mod self_alerts;
mod server_certificates;
mod server_domains;
mod server_enrollment;
mod server_group_archival;
mod server_group_version_cache;
mod server_products;
mod server_restore_window;
mod silenced_health_checks;
mod slack_outbox_enqueue;
mod snippet_name_constraint;
mod status_figures;
mod statuses_device_fk;
mod tag_reserved_prefix;
mod version_known_issue_provenance;
mod version_updates_view;
mod versions;
