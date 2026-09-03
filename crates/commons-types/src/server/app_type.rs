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
/// **The set is open.** A report is the only thing that creates an
/// application, and it carries the type, so a group can bring a new kind
/// of application to Canopy without Canopy being changed and shipped. Canopy
/// holds built-in handling for the types it knows (see [`ApplicationType::caps`]);
/// a type it does not know carries none of it and is treated generically.
///
/// There is deliberately no default. A type comes from a report, or the
/// application does not exist.
// spec: APP
#[derive(Debug, Clone, Eq, PartialEq, Hash, AsExpression)]
#[diesel(sql_type = Text)]
pub enum ApplicationType {
	/// A Tamanu central server, which facility servers sync to.
	TamanuCentral,
	/// A Tamanu facility server: an on-site instance syncing to a central.
	TamanuFacility,
	/// SENAITE, a laboratory information management system. Its instances hold
	/// no role relative to each other, so the software alone names the type.
	Senaite,
	/// A Canopy instance itself, monitored like anything else.
	Canopy,
	/// A type Canopy has no built-in handling for, named by the slug a report
	/// carried. Everything Canopy does for any application — reachability,
	/// checks, backups, figures — works for one of these; what it does not get
	/// is the per-type handling in [`ApplicationType::caps`].
	Other(String),
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
	/// The types Canopy has built-in handling for. Not every type in the
	/// fleet: an application reporting a type absent from this list is an
	/// ordinary application that carries no capabilities.
	pub const KNOWN: &'static [Self] = &[
		Self::TamanuCentral,
		Self::TamanuFacility,
		Self::Senaite,
		Self::Canopy,
	];

	/// What Canopy does for applications of this type.
	///
	/// A type Canopy does not know carries no capabilities: its version is
	/// presented as reported and graded against nothing, it is not publicly
	/// listable, and it publishes no masking manifest, so a redacting replica
	/// of one is withheld rather than served unmasked.
	pub fn caps(&self) -> Caps {
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
			Self::Other(_) => Caps {
				version_tracking: VersionTracking::Reported,
				public_listing: false,
				redaction: None,
			},
		}
	}

	/// The software this type is an instance of, without its role.
	///
	/// Cost allocation groups by software rather than by software-in-a-role, so
	/// `billing.product` reads this: a central and a facility of one group
	/// attribute to the same product, as they did when product was a field.
	// spec: APP#billing-attribution
	pub fn software(&self) -> &str {
		match self {
			Self::TamanuCentral | Self::TamanuFacility => "tamanu",
			Self::Senaite => "senaite",
			Self::Canopy => "canopy",
			// Canopy does not know how this type divides into software and
			// role, so the type names the software outright.
			Self::Other(slug) => slug,
		}
	}

	/// The role this type plays within its software, as the retired `kind`
	/// field spelled it.
	///
	/// Emitted alongside the type wherever the pair used to appear, so a
	/// client or rule reading the old shape keeps working across the
	/// transition. Nothing in Canopy decides anything on it.
	// spec: APP#where-a-type-comes-from
	pub fn role(&self) -> &'static str {
		match self {
			Self::TamanuCentral => "central",
			Self::TamanuFacility => "facility",
			// Software whose instances hold no role relative to each other:
			// standalone was only ever the absence of a kind. A type Canopy
			// does not know has no historical kind either, which is the same
			// absence.
			Self::Senaite | Self::Canopy | Self::Other(_) => "standalone",
		}
	}

	/// Whether a version is graded against a release train Canopy holds. False
	/// both for a type with no version and for one whose version is untracked.
	pub fn tracks_versions(&self) -> bool {
		matches!(self.caps().version_tracking, VersionTracking::Tracked)
	}

	/// Whether applications of this type have an application version at all.
	/// One without presents nothing, as against the `unknown` a versioned
	/// application shows before it has reported.
	pub fn has_versions(&self) -> bool {
		!matches!(self.caps().version_tracking, VersionTracking::Absent)
	}

	/// How the type reads when nothing has named the application: the type in
	/// sentence case, so a `tamanu-central` presents as "Tamanu central".
	///
	/// One rule for every type, with no per-type styling. The set is open, so
	/// a table of exceptions could only ever cover the types Canopy happens to
	/// know, and a type it does not know would read differently from one it
	/// does for no reason an operator could see.
	// spec: FLT#naming
	pub fn label(&self) -> String {
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
	/// Only known types are considered, which is what these queries want: a
	/// capability is something Canopy has built-in handling for, so a type it
	/// does not know never satisfies one and never belongs in the whitelist.
	pub fn stored_values_where(predicate: fn(&Self) -> bool) -> Vec<String> {
		Self::KNOWN
			.iter()
			.filter(|t| predicate(t))
			.map(ToString::to_string)
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
			Self::Other(slug) => write!(f, "{slug}"),
		}
	}
}

impl From<ApplicationType> for String {
	fn from(value: ApplicationType) -> Self {
		value.to_string()
	}
}

impl From<&ApplicationType> for String {
	fn from(value: &ApplicationType) -> Self {
		value.to_string()
	}
}

#[derive(Debug, Clone, thiserror::Error)]
#[error(
	"invalid application type {0:?}: a type is a slug of lowercase letters, digits and single \
	 hyphens, starting with a letter"
)]
pub struct ApplicationTypeFromStringError(String);

/// A type has to be a slug, because it is presented (as its sentence case),
/// stored, and used to address reported material. The set is open, not
/// unconstrained: anything that is not a slug is a reporting error rather than
/// a new type, and is refused at the point it arrives.
// spec: APP#where-a-type-comes-from
fn is_type_slug(s: &str) -> bool {
	let mut segments = s.split('-');
	segments.next().is_some_and(|first| {
		first.starts_with(|c: char| c.is_ascii_lowercase())
			&& first
				.chars()
				.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
	}) && s.split('-').skip(1).all(|seg| {
		!seg.is_empty()
			&& seg
				.chars()
				.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
	})
}

impl FromStr for ApplicationType {
	type Err = ApplicationTypeFromStringError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu-central" => Ok(Self::TamanuCentral),
			"tamanu-facility" => Ok(Self::TamanuFacility),
			"senaite" => Ok(Self::Senaite),
			"canopy" => Ok(Self::Canopy),
			s if is_type_slug(s) => Ok(Self::Other(s.to_owned())),
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

/// On the wire a type is its slug, whether Canopy knows it or not — so an
/// unknown type reads exactly like a known one and nothing has to be taught
/// the difference.
impl Serialize for ApplicationType {
	fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
		serializer.serialize_str(&self.to_string())
	}
}

impl<'de> Deserialize<'de> for ApplicationType {
	fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
		let raw = String::deserialize(deserializer)?;
		raw.parse().map_err(serde::de::Error::custom)
	}
}

impl utoipa::PartialSchema for ApplicationType {
	fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
		utoipa::openapi::ObjectBuilder::new()
			.schema_type(utoipa::openapi::schema::SchemaType::Type(
				utoipa::openapi::schema::Type::String,
			))
			.description(Some(
				"What an application is: the software and the role it plays, as a slug. \
				 The set is open — a report carrying a type Canopy does not know creates an \
				 application of that type, which simply carries no per-type capabilities.",
			))
			.examples([serde_json::json!("tamanu-central")])
			.build()
			.into()
	}
}

impl utoipa::ToSchema for ApplicationType {}

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
		let v = self.to_string();
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn roundtrips_through_string() {
		for t in ApplicationType::KNOWN {
			assert_eq!(t.to_string().parse::<ApplicationType>().unwrap(), *t);
		}
	}

	#[test]
	fn only_a_central_is_publicly_listable() {
		assert!(ApplicationType::TamanuCentral.caps().public_listing);
		for t in ApplicationType::KNOWN {
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

	/// One rule for every type, including ones Canopy has never heard of. No
	/// per-type styling: the set is open, so a table of exceptions could only
	/// cover the types Canopy happens to know, and an unknown type would read
	/// differently from a known one for no reason an operator could see.
	#[test]
	fn a_type_reads_as_sentence_case_when_nothing_names_it() {
		assert_eq!(ApplicationType::TamanuCentral.label(), "Tamanu central");
		assert_eq!(ApplicationType::Senaite.label(), "Senaite");
		assert_eq!(
			"open-mrs".parse::<ApplicationType>().unwrap().label(),
			"Open mrs"
		);
	}

	/// A report is the only thing that creates an application, and it carries
	/// the type, so a type Canopy has never seen has to be accepted rather than
	/// refused — otherwise no group can bring a new kind of application
	/// without Canopy being changed and shipped.
	#[test]
	fn an_unknown_type_is_accepted_and_carries_no_capabilities() {
		let t: ApplicationType = "open-mrs".parse().expect("an unknown slug is a type");
		assert_eq!(t, ApplicationType::Other("open-mrs".into()));
		assert_eq!(t.to_string(), "open-mrs");

		assert!(!t.tracks_versions(), "Canopy holds no release train for it");
		assert!(t.has_versions(), "its version is presented as reported");
		assert!(!t.caps().public_listing);
		assert!(t.caps().redaction.is_none());
		// Canopy cannot know how the type divides into software and role.
		assert_eq!(t.software(), "open-mrs");
		assert_eq!(t.role(), "standalone");
	}

	/// Open is not unconstrained. A type is presented, stored, and used to
	/// address reported material, so anything that is not a slug is a
	/// reporting error rather than a new type.
	#[test]
	fn a_type_has_to_be_a_slug() {
		for good in ["open-mrs", "openmrs", "a", "x9", "a-b-c9"] {
			assert!(good.parse::<ApplicationType>().is_ok(), "{good} is a slug");
		}
		for bad in [
			"",
			"-leading",
			"trailing-",
			"double--hyphen",
			"9leading",
			"has space",
			"Upper",
		] {
			// `Upper` lowercases before parsing, so it is the one exception:
			// case is normalised rather than refused.
			if bad == "Upper" {
				assert_eq!(
					bad.parse::<ApplicationType>().unwrap(),
					ApplicationType::Other("upper".into())
				);
				continue;
			}
			assert!(
				bad.parse::<ApplicationType>().is_err(),
				"{bad:?} is not a slug"
			);
		}
	}

	/// Only known types carry capabilities, so a capability whitelist never
	/// picks up a type Canopy has no handling for.
	#[test]
	fn a_capability_whitelist_covers_known_types_only() {
		let listable = ApplicationType::stored_values_where(|t| t.caps().public_listing);
		assert_eq!(listable, vec!["tamanu-central".to_owned()]);
	}
}
