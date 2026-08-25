//! Taking a relay's filing into canopy.
//!
//! The two families take different paths, because they are different things:
//!
//! - A **harvest** filing is a status-push body, so it goes through the very
//!   ingestion an HTTP push goes through
//!   ([`commons_servers::status_ingest`]). A Kubernetes server and a server
//!   that pushes its own reports therefore share one catalog entry and one
//!   policy per check, and cannot drift into subtly different checks — because
//!   there is one implementation, not two kept in agreement.
//! - A **substrate** filing has no push analogue (the `kubernetes` source is
//!   reserved from the device API), so it goes through `issues::file_check`,
//!   the same path canopy's own determinations take.
//!
//! Both carry the relay's device as provenance, and both land at a scope
//! expressed in the single `database::issues::Scope` vocabulary.

use commons_errors::{AppError, Result};
use commons_servers::{
	server_tags::{effective_tags_for_server, tags_for_grading},
	status_ingest,
};
use commons_types::{Uuid, source::SUBSTRATE_SOURCE};
use database::{
	diesel_async::AsyncPgConnection,
	issues::{CheckInstance, InstancedCheckFiling, Scope, file_check_instances},
	servers::Server,
};
use relay_protocol::{Filing, FilingTarget, HarvestFiling, SubstrateFiling};
use tracing::warn;

/// Where a filing lands, once the coordinates the relay named have been
/// resolved against what canopy knows.
///
/// This is the bridge from the relay's vocabulary (namespaces and instances)
/// to canopy's (servers, groups, canopy-wide). Resolution happens here and
/// nowhere else, so a relay never holds a canopy identifier.
#[derive(Debug, Clone)]
pub enum Placement {
	/// The server an instance coordinate names.
	Server(Server),
	/// The group a namespace names — a server group at a rank.
	Group(Uuid),
	/// Canopy-wide, with the relay's cluster as the check's instance.
	Cluster { label: String },
}

impl Placement {
	/// The check-state scope this placement files at.
	fn scope(&self) -> Scope {
		match self {
			Self::Server(server) => Scope::Server(server.id),
			Self::Group(id) => Scope::Group(*id),
			Self::Cluster { .. } => Scope::Global,
		}
	}
}

/// Resolve the coordinates a relay named to somewhere in canopy.
///
/// A server running on Kubernetes carries its cluster, its namespace, and (for
/// a facility) its facility identity, and an operator sets them — canopy does
/// not discover Kubernetes servers on its own (spec `K8S`, "Setting a server's
/// identity"). So this is a lookup against what the operator recorded, and a
/// coordinate that matches nothing is one filing canopy cannot place rather
/// than a relay that is out of step.
///
/// **Not yet implemented, and deliberately so.** The columns it reads —
/// a server's cluster, namespace, and facility identity — arrive with the
/// cluster registry and the identity picker. Until then every filing is
/// unplaceable, which the caller logs. This is the one function those cards
/// fill in; nothing else in the relay path needs to change for a filing to
/// start landing.
pub async fn resolve(
	_conn: &mut AsyncPgConnection,
	_relay_device_id: Uuid,
	_target: &FilingTarget,
) -> Result<Option<Placement>> {
	Ok(None)
}

/// File what a relay reported.
pub async fn ingest(
	conn: &mut AsyncPgConnection,
	relay_device_id: Uuid,
	filing: Filing,
	placement: Placement,
) -> Result<()> {
	match filing {
		Filing::Harvest(harvest) => ingest_harvest(conn, relay_device_id, harvest, placement).await,
		Filing::Substrate(substrate) => {
			ingest_substrate(conn, relay_device_id, substrate, placement).await
		}
	}
}

/// A harvested filing, through the push ingestion.
///
/// Only a server can be the subject: the harvest's subject is the thing that
/// has a database, a version, an API, and duties that ought to be running, and
/// that is one server. A harvest filing placed anywhere else is a protocol
/// error rather than something to file at a coarser grain.
async fn ingest_harvest(
	conn: &mut AsyncPgConnection,
	relay_device_id: Uuid,
	harvest: HarvestFiling,
	placement: Placement,
) -> Result<()> {
	let Placement::Server(server) = placement else {
		return Err(AppError::custom(
			"a harvest filing describes one server, so it cannot be filed at a coarser grain",
		));
	};

	// Parsed by the same parser an HTTP push goes through, so the two cannot
	// disagree about what the body means. No version header exists on this
	// path; a relay carries the version in the body as current reporters do.
	let parsed = status_ingest::parse_push(harvest.push, None)?;

	// The same effective tags a push is graded against — the relay's filing
	// must not be graded against a different tag set, or the two substrates
	// grade differently.
	let tags = tags_for_grading(&effective_tags_for_server(conn, &server).await?);

	status_ingest::ingest_push(conn, &server, relay_device_id, &parsed, &tags).await?;
	Ok(())
}

/// A substrate filing, through canopy's own filing path.
async fn ingest_substrate(
	conn: &mut AsyncPgConnection,
	relay_device_id: Uuid,
	substrate: SubstrateFiling,
	placement: Placement,
) -> Result<()> {
	let scope = placement.scope();

	// Provenance is the relay, and only where the scope is a server: it is a
	// separate concern from scope, and a group- or canopy-wide filing carries
	// none (see `CheckFiling::device_id`).
	let device_id = matches!(scope, Scope::Server(_)).then_some(relay_device_id);

	// A cluster-wide check is Canopy-wide with each cluster an instance of it,
	// so the cluster is the instance label rather than part of the check name.
	// Everything else is a single unlabelled instance, which is what
	// `file_check` files.
	let label = match &placement {
		Placement::Cluster { label } => label.clone(),
		_ => String::new(),
	};

	let message = substrate.message.clone();
	file_check_instances(
		conn,
		InstancedCheckFiling {
			source: SUBSTRATE_SOURCE,
			scope,
			device_id,
			check: &substrate.check,
			title: substrate.title.as_deref(),
			default_ceiling: substrate.default_ceiling,
			default_escalates: substrate.default_escalates,
			documentation: substrate.documentation.as_deref(),
			instances: vec![CheckInstance {
				label,
				observed: substrate.observed,
				detail: substrate.detail.clone(),
			}],
		},
		&|_| message.clone(),
	)
	.await?;

	Ok(())
}

/// A filing canopy could not place, logged rather than dropped silently.
///
/// Worth a line each time: a coordinate that resolves to nothing means an
/// operator has not told canopy which server that instance is, and the check
/// results for it are going nowhere until they do.
pub fn unplaceable(relay_device_id: Uuid, target: &FilingTarget) {
	warn!(
		relay = %relay_device_id,
		?target,
		"relay filed against coordinates canopy cannot place; no server record carries them",
	);
}
