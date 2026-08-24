//! The device-facing effective tag set for a server.
//!
//! Lives here rather than beside the `GET /tags` endpoint because check
//! grading reads it: a rule may predicate on any effective tag, so whatever
//! files a server's checks has to compute tags the same way, whether it
//! arrived as an HTTP push or as a relay filing (spec `K8S`, "Checks
//! harvested for the server"). Two computations would be two gradings.

use commons_errors::Result;
use commons_types::server::TagMap;
use database::{diesel_async::AsyncPgConnection, server_groups::ServerGroup, servers::Server};

use crate::backup_jobs::BillingLabels;

/// The device-facing effective tag set for a server: its own tags overlaid
/// on its group's, plus the synthetic read-only `canopy:` tags and the
/// effective `billing.*` labels.
pub async fn effective_tags_for_server(
	conn: &mut AsyncPgConnection,
	server: &Server,
) -> Result<TagMap> {
	let mut merged = server.tags_for_device(conn).await?;

	// Fill in the effective billing labels where the server doesn't already
	// carry one, matching what canopy attributes to the group's cloud
	// resources. `merged` already holds server tags overlaid on group tags, so a
	// stored `billing.*` tag (server's own first, then the group's) is honoured
	// and only the missing labels fall back to computed values.
	//
	// Every computed label describes *this* server, not its group: the stage
	// comes from the server's own rank, so a rank=clone server reports
	// `billing.stage=clone` and never the group's `prod`; and the product comes
	// from the server's own product, so a SENAITE server in a Tamanu group
	// reports `billing.product=senaite`. Attribution needs a deployment to
	// attribute to, so an ungrouped server carries none.
	// spec: APP#billing-attribution
	if let Some(group_id) = server.group_id {
		let group = ServerGroup::get_by_id(conn, group_id).await?;
		for (key, value) in
			BillingLabels::for_server(&group.tags, &group.name, server.product, server.rank)
				.into_tags()
		{
			merged.0.entry(key).or_insert(value);
		}
	}

	Ok(merged)
}

/// The effective tags as check grading reads them: a rule sees each tag as
/// `tags.<key>`, uniformly with the JSON extras it also matches on.
pub fn tags_for_grading(tags: &TagMap) -> std::collections::HashMap<String, serde_json::Value> {
	tags.0
		.iter()
		.map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
		.collect()
}
