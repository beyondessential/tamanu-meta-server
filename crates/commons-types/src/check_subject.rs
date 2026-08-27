//! What a check asserts something about: the machine, or the application.
//!
//! A check's *subject* is not the same question as what a check needs in order
//! to run. bestool's registry categorises by inputs (`@tamanu`, `@db`, `host`),
//! which is why the subject has to be decided here rather than read off the
//! wire: a check that needs the host to run may still be describing the
//! application on it.
//!
//! `disk_free`, `memory`, `load`, `time_sync`, `tailscale` and their like are
//! not application checks that happen to run on a host; they assert something
//! about the box. Canopy filed them against an application only because an
//! application was the only target available.
// spec: STA

use serde::{Deserialize, Serialize};

/// The grain a check's result belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckSubject {
	/// The box: its disk, memory, clock, addresses, agent.
	Machine,
	/// The workload: its version, its database, its own health.
	Application,
}

/// The checks that describe the box rather than the workload on it.
///
/// **Whole names, never prefixes.** Caddy straddles the split — `caddy_certs`
/// is an application's while the rest describe the install — and `ips` and
/// `ips_errors` share a prefix and nothing else, one being the machine's
/// addresses and the other a Tamanu error stream. A prefix rule files both
/// wrongly, and silently.
///
/// Canopy holds this list only while unified pushes exist. A reporter that
/// states the subject itself makes it redundant; until then, a check named
/// here from any source is the machine's.
const MACHINE_SUBJECT_CHECKS: &[&str] = &[
	"billing_tags",
	"btrfs",
	"caddy_resolvers",
	"caddy_version",
	"caddyfile_version",
	"canopy_registration",
	"disk_free",
	"external_users",
	"held_captures",
	"inodes",
	"ips",
	"load",
	"memory",
	"munin",
	"tailscale",
	"tailscale_config",
	"time_sync",
	"uptime",
];

impl CheckSubject {
	/// What this check name asserts about. Anything not named as the machine's
	/// is the application's: an unrecognised check is far more likely to be a
	/// product's own than a new fact about the box, and filing it against the
	/// application keeps it where every check has always gone.
	pub fn of(check_name: &str) -> Self {
		if MACHINE_SUBJECT_CHECKS.contains(&check_name) {
			Self::Machine
		} else {
			Self::Application
		}
	}

	pub fn is_machine(self) -> bool {
		matches!(self, Self::Machine)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_split_matches_whole_names_not_prefixes() {
		// Caddy straddles it: the install is the box's, the certificate the
		// application's.
		assert_eq!(CheckSubject::of("caddy_version"), CheckSubject::Machine);
		assert_eq!(CheckSubject::of("caddy_resolvers"), CheckSubject::Machine);
		assert_eq!(
			CheckSubject::of("caddy_certs"),
			CheckSubject::Application,
			"a certificate is served by an application, not by the box"
		);

		// Shared prefix, unrelated subjects.
		assert_eq!(CheckSubject::of("ips"), CheckSubject::Machine);
		assert_eq!(
			CheckSubject::of("ips_errors"),
			CheckSubject::Application,
			"an error stream is the product's, not the box's addresses"
		);
	}

	#[test]
	fn anything_unrecognised_is_the_applications() {
		assert_eq!(CheckSubject::of("postgres"), CheckSubject::Application);
		assert_eq!(CheckSubject::of("tasks"), CheckSubject::Application);
		assert_eq!(
			CheckSubject::of("some_check_invented_tomorrow"),
			CheckSubject::Application
		);
	}

	#[test]
	fn the_machine_list_is_sorted_and_unique() {
		// It is read by eye when a check moves grain; keep it scannable.
		let mut sorted = MACHINE_SUBJECT_CHECKS.to_vec();
		sorted.sort_unstable();
		assert_eq!(
			MACHINE_SUBJECT_CHECKS,
			&sorted[..],
			"keep the machine-subject list in alphabetical order"
		);
		sorted.dedup();
		assert_eq!(
			MACHINE_SUBJECT_CHECKS.len(),
			sorted.len(),
			"a check names its subject once"
		);
	}
}
