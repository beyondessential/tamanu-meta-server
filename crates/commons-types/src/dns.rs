//! Managed DNS zones and domain-name handling.
//!
//! A [`ManagedZone`] is a DNS zone Canopy has write access to, declared by the
//! infrastructure that provisions Canopy rather than by an operator. Group
//! domain claims are validated against the configured zones: a claim has to sit
//! at or under one apex, and resolves to the longest apex it sits under.
// spec: DOM

use commons_errors::{AppError, Result};
use serde::{Deserialize, Serialize};

/// Environment variable declaring the zones Canopy may write records in.
pub const ZONES_ENV: &str = "CANOPY_DNS_ZONES";

/// Environment variable declaring the role to assume for zones that don't name
/// one of their own.
pub const ZONE_ROLE_ENV: &str = "CANOPY_DNS_ZONE_ROLE_ARN";

/// The longest a domain name may be, in its presentation form without the root
/// dot.
const NAME_MAX: usize = 253;
const LABEL_MAX: usize = 63;

/// A DNS zone Canopy can create, change, and delete records in.
///
/// Zones are shared: several groups' domains live in one zone, and names Canopy
/// doesn't manage at all may live there beside them — so a zone is never owned
/// by a group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ManagedZone {
	/// The zone's apex domain, normalised: lower case, no trailing dot.
	pub apex: String,
	/// The identifier the DNS provider knows this zone by (for Route 53, the
	/// hosted zone id).
	pub provider_zone_id: String,
	/// The role to assume to reach this zone, when the zone lives in another
	/// account than Canopy's own.
	pub role_arn: Option<String>,
}

impl ManagedZone {
	/// Parse the zone list from the environment. `Ok(empty)` when unset or
	/// blank — the features that need a zone report the misconfiguration
	/// themselves rather than keeping Canopy from starting; `Err` when set but
	/// unparseable.
	pub fn list_from_env() -> Result<Vec<Self>> {
		let raw = std::env::var(ZONES_ENV).unwrap_or_default();
		let default_role = std::env::var(ZONE_ROLE_ENV)
			.ok()
			.map(|r| r.trim().to_string())
			.filter(|r| !r.is_empty());
		Self::parse_list(&raw, default_role.as_deref())
	}

	/// Parse a zone list of `apex=provider-zone-id[=role-arn]` entries separated
	/// by commas or whitespace. An entry that names no role takes
	/// `default_role`.
	pub fn parse_list(raw: &str, default_role: Option<&str>) -> Result<Vec<Self>> {
		let mut zones: Vec<Self> = Vec::new();
		for entry in raw.split([',', ' ', '\t', '\n', '\r']) {
			let entry = entry.trim();
			if entry.is_empty() {
				continue;
			}

			let mut parts = entry.split('=');
			let apex = normalize_domain(parts.next().unwrap_or_default())?;
			let provider_zone_id = parts.next().unwrap_or_default().trim();
			if provider_zone_id.is_empty() {
				return Err(AppError::BadRequest(format!(
					"managed zone {apex:?} has no provider zone id: expected `apex=provider-zone-id[=role-arn]`"
				)));
			}
			let role_arn = parts
				.next()
				.map(str::trim)
				.filter(|r| !r.is_empty())
				.map(str::to_string)
				.or_else(|| default_role.map(str::to_string));
			if parts.next().is_some() {
				return Err(AppError::BadRequest(format!(
					"managed zone entry {entry:?} has too many fields: expected `apex=provider-zone-id[=role-arn]`"
				)));
			}

			if zones.iter().any(|z| z.apex == apex) {
				return Err(AppError::BadRequest(format!(
					"managed zone {apex:?} is declared more than once"
				)));
			}
			zones.push(Self {
				apex,
				provider_zone_id: provider_zone_id.to_string(),
				role_arn,
			});
		}
		Ok(zones)
	}
}

/// Normalise a domain name to the form Canopy holds it in: lower case, no
/// trailing dot, no surrounding whitespace.
///
/// Rejects anything that isn't a syntactically valid name of at least two
/// labels. Names are handled in the form DNS carries them, so an
/// internationalised domain has to arrive as its ASCII-compatible (punycode)
/// spelling.
pub fn normalize_domain(input: &str) -> Result<String> {
	let bad = |reason: &str| {
		Err(AppError::BadRequest(format!(
			"invalid domain name {input:?}: {reason}"
		)))
	};

	let name = input.trim().trim_end_matches('.').to_ascii_lowercase();
	if name.is_empty() {
		return bad("it is empty");
	}
	if name.len() > NAME_MAX {
		return bad(&format!("it is longer than {NAME_MAX} characters"));
	}
	let labels: Vec<&str> = name.split('.').collect();
	if labels.len() < 2 {
		return bad("it has only one label");
	}
	for label in labels {
		if label.is_empty() {
			return bad("it has an empty label");
		}
		if label.len() > LABEL_MAX {
			return bad(&format!(
				"label {label:?} is longer than {LABEL_MAX} characters"
			));
		}
		if label.starts_with('-') || label.ends_with('-') {
			return bad(&format!("label {label:?} starts or ends with a hyphen"));
		}
		if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
			return bad(&format!(
				"label {label:?} has characters outside letters, digits, and hyphens"
			));
		}
	}

	Ok(name)
}

/// Whether `name` is at or beneath `apex`. Both must already be normalised.
pub fn is_within(name: &str, apex: &str) -> bool {
	name == apex
		|| name
			.strip_suffix(apex)
			.is_some_and(|rest| rest.ends_with('.'))
}

/// Whether two claims overlap: the same name, or one at or beneath the other.
/// Both must already be normalised.
pub fn overlaps(a: &str, b: &str) -> bool {
	is_within(a, b) || is_within(b, a)
}

/// The managed zone a name resolves to: the longest configured apex the name
/// sits within, so a zone configured beneath another zone takes the names
/// beneath itself. `None` when no configured zone covers the name.
pub fn match_zone<'z>(name: &str, zones: &'z [ManagedZone]) -> Option<&'z ManagedZone> {
	zones
		.iter()
		.filter(|zone| is_within(name, &zone.apex))
		.max_by_key(|zone| zone.apex.len())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn normalises_case_and_trailing_dot() {
		assert_eq!(
			normalize_domain(" Fiji.Tamanu.App. ").unwrap(),
			"fiji.tamanu.app"
		);
	}

	#[test]
	fn rejects_malformed_names() {
		for input in [
			"",
			".",
			"app",
			"fiji..app",
			"-fiji.app",
			"fiji-.app",
			"fiji_1.app",
			"fidʒi.app",
			"*.tamanu.app",
			&format!("{}.app", "a".repeat(64)),
		] {
			assert!(
				normalize_domain(input).is_err(),
				"expected {input:?} to be rejected"
			);
		}
	}

	#[test]
	fn within_needs_a_label_boundary() {
		assert!(is_within("tamanu.app", "tamanu.app"));
		assert!(is_within("fiji.tamanu.app", "tamanu.app"));
		assert!(!is_within("nottamanu.app", "tamanu.app"));
		assert!(!is_within("tamanu.app", "fiji.tamanu.app"));
	}

	#[test]
	fn overlap_is_symmetric() {
		assert!(overlaps("fiji.tamanu.app", "tamanu.app"));
		assert!(overlaps("tamanu.app", "fiji.tamanu.app"));
		assert!(overlaps("fiji.tamanu.app", "fiji.tamanu.app"));
		assert!(!overlaps("fiji.tamanu.app", "samoa.tamanu.app"));
	}

	#[test]
	fn longest_apex_wins() {
		let zones = ManagedZone::parse_list("tamanu.app=Z1, demo.tamanu.app=Z2", None).unwrap();
		assert_eq!(
			match_zone("x.demo.tamanu.app", &zones)
				.unwrap()
				.provider_zone_id,
			"Z2"
		);
		assert_eq!(
			match_zone("x.prod.tamanu.app", &zones)
				.unwrap()
				.provider_zone_id,
			"Z1"
		);
		assert!(match_zone("x.senaite.app", &zones).is_none());
	}

	#[test]
	fn parses_roles_and_defaults() {
		let zones = ManagedZone::parse_list(
			"Tamanu.App.=Z1=arn:aws:iam::1:role/own\nsenaite.app=Z2",
			Some("arn:aws:iam::9:role/default"),
		)
		.unwrap();
		assert_eq!(
			zones,
			vec![
				ManagedZone {
					apex: "tamanu.app".into(),
					provider_zone_id: "Z1".into(),
					role_arn: Some("arn:aws:iam::1:role/own".into()),
				},
				ManagedZone {
					apex: "senaite.app".into(),
					provider_zone_id: "Z2".into(),
					role_arn: Some("arn:aws:iam::9:role/default".into()),
				},
			]
		);
	}

	#[test]
	fn rejects_bad_config() {
		assert!(ManagedZone::parse_list("tamanu.app", None).is_err());
		assert!(ManagedZone::parse_list("tamanu.app=", None).is_err());
		assert!(ManagedZone::parse_list("tamanu.app=Z1=r=extra", None).is_err());
		assert!(ManagedZone::parse_list("tamanu.app=Z1,tamanu.app=Z2", None).is_err());
		assert!(ManagedZone::parse_list("", None).unwrap().is_empty());
	}
}
