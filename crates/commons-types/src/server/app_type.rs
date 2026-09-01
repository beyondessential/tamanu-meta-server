use std::{fmt::Display, str::FromStr};

use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// What an application is: the software and the role it plays, together.
///
/// A Tamanu central and a Tamanu facility are two types rather than one type
/// in two configurations. They behave differently — a large set of checks
/// exists only on centrals and another only on facilities — which is not how
/// two instances of one thing behave.
///
/// The set is closed and defined here rather than configured, because each
/// type's handling is built in. See [`ApplicationType::caps`].
// spec: APP
#[derive(
	Debug,
	Clone,
	Copy,
	Default,
	Eq,
	PartialEq,
	Hash,
	Serialize,
	Deserialize,
	AsExpression,
	utoipa::ToSchema,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "kebab-case")]
pub enum ApplicationType {
	/// A Tamanu central server, which facility servers sync to. The default.
	#[default]
	TamanuCentral,
	/// A Tamanu facility server: an on-site instance syncing to a central.
	TamanuFacility,
	/// SENAITE, a laboratory information management system. Its instances hold
	/// no role relative to each other, so the software alone names the type.
	Senaite,
	/// A Canopy instance itself, monitored like anything else.
	Canopy,
}

/// How Canopy treats an application's version.
// spec: APP#capabilities
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum VersionTracking {
	/// Canopy holds this type's release train, so a version is graded against
	/// it: how far behind, what updates are available, which known issues apply.
	Tracked,
	/// The application reports a version Canopy holds no release train for, so
	/// it stands as reported and is graded against nothing.
	Reported,
	/// The type has no application version at all.
	Absent,
}

/// Where a type publishes the masking manifests that say how to de-identify a
/// restored copy of one of its databases.
///
/// Canopy holds this rather than the operator: what to mask is a property of
/// the software, so an operator declaring a redacting replica says only that it
/// redacts, never where its masking comes from.
// spec: RST#the-masking-manifest
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, utoipa::ToSchema)]
pub struct RedactionManifest {
	/// Where the manifest for a given version lives. `{version}` is substituted
	/// by the consumer with the version it reads out of the data it restored.
	pub url_template: &'static str,
	/// Single-row, single-column SQL reading the group's own version out of the
	/// restored database, to substitute into `url_template`.
	pub version_query: &'static str,
	/// Whether to retry at the `major.minor.0` base version when the versioned
	/// URL 404s, for software publishing per minor rather than per patch.
	pub fallback_to_base: bool,
	/// The artefact type under which a version's manifest is registered, so
	/// Canopy can tell whether the URL it would hand out has actually been
	/// published for that version.
	pub artifact_type: &'static str,
}

impl RedactionManifest {
	/// The manifest URL for a concrete version, substituting `{version}`.
	///
	/// Canopy uses this to corroborate the template against what a version
	/// actually published; the consumer does its own substitution against the
	/// version it reads out of the restored data, which may differ from the one
	/// the application reports running.
	pub fn url_for(&self, version: &node_semver::Version) -> String {
		self.url_template.replace("{version}", &version.to_string())
	}
}

/// Where Tamanu's dbt docs deploy publishes each version's manifest.
const TAMANU_MASKING_MANIFEST_URL: &str =
	"https://docs.data.bes.au/tamanu/v{version}/manifest.json";

const TAMANU_REDACTION: RedactionManifest = RedactionManifest {
	url_template: TAMANU_MASKING_MANIFEST_URL,
	version_query: "SELECT value FROM local_system_facts WHERE key = 'currentVersion'",
	fallback_to_base: true,
	artifact_type: "dbt-manifest",
};

/// What Canopy does for an application of a given type.
///
/// Reachability, health checks and backups are deliberately absent: checks are
/// graded by the source that reports them, and backup types are advertised per
/// machine by the agent, so both already work for any type.
// spec: APP#capabilities
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Caps {
	/// How this type's application version is treated.
	pub version_tracking: VersionTracking,
	/// Whether applications of this type can be listed for end-user-facing
	/// clients.
	pub public_listing: bool,
	/// Where this type's masking manifests live, when it publishes them. A type
	/// without one cannot have a redacting replica restored: the worklist
	/// withholds the entry rather than serving unmasked data.
	pub redaction: Option<RedactionManifest>,
}

impl ApplicationType {
	pub const ALL: &'static [Self] = &[
		Self::TamanuCentral,
		Self::TamanuFacility,
		Self::Senaite,
		Self::Canopy,
	];

	/// What Canopy does for applications of this type.
	pub const fn caps(self) -> Caps {
		match self {
			// Only a central is publicly listable. A facility sits behind
			// someone else's NAT and is nobody's to look up.
			Self::TamanuCentral => Caps {
				version_tracking: VersionTracking::Tracked,
				public_listing: true,
				redaction: Some(TAMANU_REDACTION),
			},
			Self::TamanuFacility => Caps {
				version_tracking: VersionTracking::Tracked,
				public_listing: false,
				redaction: Some(TAMANU_REDACTION),
			},
			// A Canopy instance reports its own build version, which Canopy
			// holds no release train for: presented, but graded against nothing.
			Self::Canopy => Caps {
				version_tracking: VersionTracking::Reported,
				public_listing: false,
				redaction: None,
			},
			Self::Senaite => Caps {
				version_tracking: VersionTracking::Absent,
				public_listing: false,
				redaction: None,
			},
		}
	}

	/// The software this type is an instance of, without its role.
	///
	/// Cost allocation groups by software rather than by software-in-a-role, so
	/// `billing.product` reads this: a central and a facility of one deployment
	/// attribute to the same product, as they did when product was a field.
	// spec: APP#billing-attribution
	pub const fn software(self) -> &'static str {
		match self {
			Self::TamanuCentral | Self::TamanuFacility => "tamanu",
			Self::Senaite => "senaite",
			Self::Canopy => "canopy",
		}
	}

	/// The role this type plays within its software, as the retired `kind`
	/// field spelled it.
	///
	/// Emitted alongside the type wherever the pair used to appear, so a
	/// client or rule reading the old shape keeps working across the
	/// transition. Nothing in Canopy decides anything on it.
	// spec: APP#where-a-type-comes-from
	pub const fn role(self) -> &'static str {
		match self {
			Self::TamanuCentral => "central",
			Self::TamanuFacility => "facility",
			// Software whose instances hold no role relative to each other:
			// standalone was only ever the absence of a kind.
			Self::Senaite | Self::Canopy => "standalone",
		}
	}

	/// Whether a version is graded against a release train Canopy holds. False
	/// both for a type with no version and for one whose version is untracked.
	pub const fn tracks_versions(self) -> bool {
		matches!(self.caps().version_tracking, VersionTracking::Tracked)
	}

	/// Whether applications of this type have an application version at all.
	/// One without presents nothing, as against the `unknown` a versioned
	/// application shows before it has reported.
	pub const fn has_versions(self) -> bool {
		!matches!(self.caps().version_tracking, VersionTracking::Absent)
	}

	/// How the type reads when nothing has named the application: the type in
	/// sentence case, so a `tamanu-central` presents as "Tamanu central".
	// spec: FLT#naming
	pub fn label(self) -> String {
		let raw = self.to_string().replace('-', " ");
		let mut chars = raw.chars();
		match chars.next() {
			Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
			None => raw,
		}
	}

	/// The stored values of every type satisfying `predicate`, for filtering a
	/// query on a capability rather than restating which types happen to have
	/// it. Keeps [`Self::caps`] the only place the mapping lives, so a new type
	/// is picked up by every such query at once.
	pub fn stored_values_where(predicate: fn(Self) -> bool) -> Vec<String> {
		Self::ALL
			.iter()
			.copied()
			.filter(|t| predicate(*t))
			.map(String::from)
			.collect()
	}
}

impl Display for ApplicationType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::TamanuCentral => write!(f, "tamanu-central"),
			Self::TamanuFacility => write!(f, "tamanu-facility"),
			Self::Senaite => write!(f, "senaite"),
			Self::Canopy => write!(f, "canopy"),
		}
	}
}

impl From<ApplicationType> for String {
	fn from(value: ApplicationType) -> Self {
		value.to_string()
	}
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid application type: {0}")]
pub struct ApplicationTypeFromStringError(String);

impl FromStr for ApplicationType {
	type Err = ApplicationTypeFromStringError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu-central" => Ok(Self::TamanuCentral),
			"tamanu-facility" => Ok(Self::TamanuFacility),
			"senaite" => Ok(Self::Senaite),
			"canopy" => Ok(Self::Canopy),
			s => Err(ApplicationTypeFromStringError(s.into())),
		}
	}
}

impl TryFrom<String> for ApplicationType {
	type Error = ApplicationTypeFromStringError;
	fn try_from(value: String) -> Result<Self, Self::Error> {
		value.parse()
	}
}

impl<DB> FromSql<Text, DB> for ApplicationType
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(Self::try_from(s)?)
	}
}

impl ToSql<Text, diesel::pg::Pg> for ApplicationType
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrips_through_string() {
		for t in ApplicationType::ALL {
			assert_eq!(t.to_string().parse::<ApplicationType>().unwrap(), *t);
		}
	}

	#[test]
	fn only_a_central_is_publicly_listable() {
		assert!(ApplicationType::TamanuCentral.caps().public_listing);
		for t in ApplicationType::ALL {
			if *t != ApplicationType::TamanuCentral {
				assert!(!t.caps().public_listing, "{t} should not be listable");
			}
		}
	}

	#[test]
	fn both_tamanu_types_are_graded_against_the_release_train() {
		assert!(ApplicationType::TamanuCentral.tracks_versions());
		assert!(ApplicationType::TamanuFacility.tracks_versions());
		assert!(!ApplicationType::Canopy.tracks_versions());
		assert!(!ApplicationType::Senaite.tracks_versions());
	}

	/// Cost allocation groups by software, so a central and a facility bill to
	/// one product. This is what `billing.product` carried when product was a
	/// field of its own.
	#[test]
	fn both_tamanu_types_bill_to_one_software() {
		assert_eq!(ApplicationType::TamanuCentral.software(), "tamanu");
		assert_eq!(ApplicationType::TamanuFacility.software(), "tamanu");
		assert_eq!(ApplicationType::Senaite.software(), "senaite");
	}

	/// Both Tamanu types restore the same database, so both redact the same way.
	#[test]
	fn both_tamanu_types_share_a_masking_manifest() {
		assert_eq!(
			ApplicationType::TamanuCentral.caps().redaction,
			ApplicationType::TamanuFacility.caps().redaction
		);
	}

	#[test]
	fn a_type_reads_as_sentence_case_when_nothing_names_it() {
		assert_eq!(ApplicationType::TamanuCentral.label(), "Tamanu central");
		assert_eq!(ApplicationType::Senaite.label(), "Senaite");
	}
}
