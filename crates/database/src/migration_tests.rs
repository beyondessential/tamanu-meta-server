//! Which versions Canopy asks to be migration-tested against which servers.
//!
//! The version axis only. Whether a server has a snapshot to restore and
//! migrate is settled when a consumer's worklist is built.

use std::collections::BTreeMap;

use commons_errors::Result;
use commons_types::{
	server::product::Product,
	version::{VersionStatus, VersionStr},
};
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use crate::{reported_detail::ReportedDetail, servers::Server, versions::Version};

/// A version a server could upgrade to, so one to test against that server's
/// data before it gets there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
	pub server_id: Uuid,
	pub version_id: Uuid,
}

/// The published versions `reported` could upgrade to, one per minor series.
///
/// Stays within the reported major, matching the update path Canopy serves a
/// server. Only the newest patch of each minor is returned: an upgrade applies
/// that patch, so an earlier one is never what the server ends up running, and
/// testing it would answer a question nobody asks.
// spec: RST#candidate-versions
pub fn upgrade_path(reported: &VersionStr, versions: &[Version]) -> Vec<Uuid> {
	let current = &reported.0;
	let mut newest_per_minor: BTreeMap<i32, &Version> = BTreeMap::new();

	for version in versions {
		if version.status != VersionStatus::Published || version.major != current.major as i32 {
			continue;
		}

		let is_newer = version.minor > current.minor as i32
			|| (version.minor == current.minor as i32 && version.patch > current.patch as i32);
		if !is_newer {
			continue;
		}

		newest_per_minor
			.entry(version.minor)
			.and_modify(|held| {
				if version.patch > held.patch {
					*held = version;
				}
			})
			.or_insert(version);
	}

	newest_per_minor
		.into_values()
		.map(|version| version.id)
		.collect()
}

/// Every candidate across the fleet.
///
/// Tamanu servers only: the migrations under test are Tamanu's, so no other
/// product's server has an upgrade path through them.
// spec: RST#candidate-versions
pub async fn candidates(db: &mut AsyncPgConnection) -> Result<Vec<Candidate>> {
	let versions = Version::get_all(db).await?;
	let mut candidates = Vec::new();

	for server in Server::get_all(db, 0, None).await? {
		if server.product != Product::Tamanu {
			continue;
		}

		let Some(reported) = ReportedDetail::last_version(db, server.id).await? else {
			continue;
		};

		candidates.extend(
			upgrade_path(&reported, &versions)
				.into_iter()
				.map(|version_id| Candidate {
					server_id: server.id,
					version_id,
				}),
		);
	}

	Ok(candidates)
}
