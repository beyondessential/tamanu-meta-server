//! Periodic checks driven by the cached `TailnetDirectory`. Each sweep
//! reconciles the directory's view of a node against canopy's
//! persisted device rows and files / closes the relevant issues.

use commons_errors::Result;
use commons_types::{Uuid, status::CheckResult};
use database::{
	devices::Device,
	issues::{CheckFiling, FilingScope, Issue, file_check},
};
use diesel_async::AsyncPgConnection;

use crate::tailnet_directory::TailnetDirectory;

/// Source value canopy uses when it files Tailscale-driven issues.
pub const TAILSCALE_SOURCE: &str = "canopy";

/// Ref value for the one "tailnet key will expire" issue per
/// `(server, device)` pair.
pub const KEY_EXPIRY_REF: &str = "tailscale-key-expiry";

/// Sweep every tailnet-attached device that's wired to at least one
/// server, and file (or close) the key-expiry check per `(server,
/// device)` pair based on the node's `keyExpiryDisabled`. The check
/// registers as an escalating failure — losing contact with a node is
/// stop-the-world — but operators can regrade it from the catalog.
///
/// Tailnet-attached devices with no server are intentionally skipped:
/// the tailnet hosts plenty of nodes that aren't canopy-managed
/// servers (operator laptops, other infra, …) and we have nothing to
/// say about those — the sweep is scoped to the headless devices
/// canopy actually runs.
///
/// Returns the number of events filed in this pass.
pub async fn sweep_key_expiry(
	db: &mut AsyncPgConnection,
	directory: &TailnetDirectory,
) -> Result<usize> {
	let pairs = Device::list_tailnet_attached_with_server(db).await?;
	if pairs.is_empty() {
		return Ok(0);
	}

	let snapshot = directory.snapshot_by_node_id().await;
	let server_ids: Vec<Uuid> = pairs.iter().map(|(_, s, _)| *s).collect();
	let existing =
		Issue::list_by_source_ref(db, TAILSCALE_SOURCE, KEY_EXPIRY_REF, &server_ids).await?;
	// `list_by_source_ref` is filtered by `server_ids`, so every row is
	// server-scoped (`server_id` is `Some`); drop any defensively.
	let issue_map: std::collections::HashMap<Uuid, &Issue> = existing
		.iter()
		.filter_map(|i| i.server_id.map(|sid| (sid, i)))
		.collect();

	let mut filed = 0usize;
	for (device, server_id, node_id) in &pairs {
		let entry = snapshot.get(node_id);
		let existing_issue = issue_map.get(server_id).copied();

		// Node not in the directory (left the tailnet / not yet
		// refreshed) — leave any existing issue alone and don't file a
		// new one. Detach is the operator action that would actually
		// clear this state.
		let Some(entry) = entry else {
			continue;
		};

		let (observed, title, message) = match (entry.key_expiry_disabled, existing_issue) {
			// Healthy state, no issue: nothing to do.
			(true, None) => continue,
			// Healthy state, issue already inactive: nothing to do.
			(true, Some(issue)) if !issue.active => continue,
			// Healthy state, but an active issue exists: close it.
			(true, Some(_)) => (
				CheckResult::Passed,
				None,
				format!("Tailscale key expiry disabled for {}", entry.node_name),
			),
			// Unhealthy state: file or refresh the failure.
			(false, _) => (
				CheckResult::Failed,
				Some(format!("Tailscale key will expire for {}", entry.node_name)),
				format!(
					"Tailnet node {} ({}) has key expiry enabled. When the \
					 node's key expires, it will drop off the tailnet and \
					 canopy will lose contact.",
					entry.node_name, entry.node_id,
				),
			),
		};

		file_check(
			db,
			CheckFiling {
				source: TAILSCALE_SOURCE,
				scope: FilingScope::Server {
					server_id: *server_id,
					device_id: Some(device.id),
				},
				check: KEY_EXPIRY_REF,
				observed,
				title: title.as_deref(),
				message: &message,
				detail: Some(serde_json::json!({
					"node_id": entry.node_id,
					"node_name": entry.node_name,
				})),
				default_ceiling: CheckResult::Failed,
				default_escalates: true,
			},
		)
		.await?;
		filed += 1;
	}

	Ok(filed)
}
