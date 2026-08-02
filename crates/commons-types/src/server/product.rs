use std::{fmt::Display, str::FromStr};

use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

use super::kind::ServerKind;

/// Which application a server runs.
///
/// The set is closed and defined here rather than configured, because each
/// product's handling — what canopy tracks for it, what it presents — is
/// built in. See [`Product::caps`].
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
#[serde(rename_all = "lowercase")]
pub enum Product {
	/// Tamanu, the product canopy was built to monitor. The default.
	#[default]
	Tamanu,
	/// SENAITE, a laboratory information management system.
	Senaite,
	/// A canopy instance itself, monitored like any other server.
	Canopy,
}

/// How canopy treats a product's application version.
// spec: APP#versions
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum VersionTracking {
	/// Canopy holds this product's release train, so a server's version is
	/// graded against it: how far behind, what updates are available, which
	/// known issues apply.
	Tracked,
	/// The server reports a version canopy holds no release train for, so it
	/// stands as reported and is graded against nothing.
	Reported,
	/// The product has no application version at all.
	Absent,
}

/// Where a product publishes the masking manifests that say how to
/// de-identify a restored copy of one of its databases.
///
/// Canopy holds this rather than the operator: what to mask is a property
/// of the product, so an operator declaring a redacting replica says only
/// that it redacts, never where its masking comes from.
// spec: RST#the-masking-manifest
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, utoipa::ToSchema)]
pub struct RedactionManifest {
	/// Where the manifest for a given version lives. `{version}` is
	/// substituted by the consumer with the version it reads out of the
	/// data it restored.
	pub url_template: &'static str,
	/// Single-row, single-column SQL reading the deployment's own version
	/// out of the restored database, to substitute into `url_template`.
	pub version_query: &'static str,
	/// Whether to retry at the `major.minor.0` base version when the
	/// versioned URL 404s, for a product publishing per minor rather than
	/// per patch.
	pub fallback_to_base: bool,
	/// The artefact type under which a version's manifest is registered, so
	/// canopy can tell whether the URL it would hand out has actually been
	/// published for that version.
	pub artifact_type: &'static str,
}

impl RedactionManifest {
	/// The manifest URL for a concrete version, substituting `{version}`.
	///
	/// Canopy uses this to corroborate the template against what a version
	/// actually published; the consumer does its own substitution against
	/// the version it reads out of the restored data, which may differ from
	/// the one the server reports running.
	pub fn url_for(&self, version: &node_semver::Version) -> String {
		self.url_template.replace("{version}", &version.to_string())
	}
}

/// Where Tamanu's dbt docs deploy publishes each version's manifest.
const TAMANU_MASKING_MANIFEST_URL: &str =
	"https://docs.data.bes.au/tamanu/v{version}/manifest.json";

/// What canopy does for a product's servers.
///
/// Reachability, health checks and backups are deliberately absent: checks
/// are graded by the source that reports them, and backup types are
/// advertised per-server by the agent, so both already work for any product.
// spec: APP#capabilities
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, utoipa::ToSchema)]
pub struct Caps {
	/// How this product's application version is treated.
	pub version_tracking: VersionTracking,
	/// Whether this product's servers can be listed for end-user-facing
	/// clients.
	pub public_listing: bool,
	/// Where this product's masking manifests live, when it publishes them.
	/// A product without one cannot have a redacting replica restored: the
	/// worklist withholds the entry rather than serving unmasked data.
	pub redaction: Option<RedactionManifest>,
}

impl Product {
	pub const ALL: &'static [Self] = &[Self::Tamanu, Self::Senaite, Self::Canopy];

	/// What canopy does for this product's servers.
	pub const fn caps(self) -> Caps {
		match self {
			Self::Tamanu => Caps {
				version_tracking: VersionTracking::Tracked,
				public_listing: true,
				redaction: Some(RedactionManifest {
					url_template: TAMANU_MASKING_MANIFEST_URL,
					version_query: "SELECT value FROM local_system_facts WHERE key = 'currentVersion'",
					fallback_to_base: true,
					artifact_type: "dbt-manifest",
				}),
			},
			// A canopy instance reports its own build version, which canopy
			// holds no release train for: presented, but graded against
			// nothing.
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

	/// Whether a server's version is graded against a release train canopy
	/// holds. False both for a product with no version and for one whose
	/// version canopy doesn't track.
	pub const fn tracks_versions(self) -> bool {
		matches!(self.caps().version_tracking, VersionTracking::Tracked)
	}

	/// Whether this product's servers have an application version at all.
	/// A server of a product without one presents nothing, as against the
	/// `unknown` a versioned server shows before it has reported.
	pub const fn has_versions(self) -> bool {
		!matches!(self.caps().version_tracking, VersionTracking::Absent)
	}

	/// The kinds this product defines, in the order they rank for a group's
	/// canonical member.
	pub const fn kinds(self) -> &'static [ServerKind] {
		match self {
			Self::Tamanu => &[ServerKind::Central, ServerKind::Facility],
			Self::Senaite | Self::Canopy => &[ServerKind::Standalone],
		}
	}

	/// The kind a server of this product takes when none is chosen, and the
	/// one it moves to when reclassifying leaves its current kind undefined
	/// for the new product.
	pub const fn default_kind(self) -> ServerKind {
		match self {
			Self::Tamanu => ServerKind::Central,
			Self::Senaite | Self::Canopy => ServerKind::Standalone,
		}
	}

	/// Whether this product defines `kind` as one of its roles.
	pub fn defines_kind(self, kind: ServerKind) -> bool {
		self.kinds().contains(&kind)
	}

	/// The stored values of every product satisfying `predicate`, for
	/// filtering a query on a capability rather than restating which products
	/// happen to have it. Keeps [`Self::caps`] the only place the mapping
	/// lives, so a new product is picked up by every such query at once.
	pub fn stored_values_where(predicate: fn(Self) -> bool) -> Vec<String> {
		Self::ALL
			.iter()
			.copied()
			.filter(|p| predicate(*p))
			.map(String::from)
			.collect()
	}
}

impl Display for Product {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Product::Tamanu => write!(f, "tamanu"),
			Product::Senaite => write!(f, "senaite"),
			Product::Canopy => write!(f, "canopy"),
		}
	}
}

impl From<Product> for String {
	fn from(product: Product) -> Self {
		product.to_string()
	}
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid server product: {0}")]
pub struct ProductFromStringError(String);

impl FromStr for Product {
	type Err = ProductFromStringError;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		match value.to_ascii_lowercase().as_ref() {
			"tamanu" => Ok(Self::Tamanu),
			"senaite" => Ok(Self::Senaite),
			"canopy" => Ok(Self::Canopy),
			s => Err(ProductFromStringError(s.into())),
		}
	}
}

impl TryFrom<String> for Product {
	type Error = ProductFromStringError;
	fn try_from(value: String) -> Result<Self, Self::Error> {
		value.parse()
	}
}

impl<DB> FromSql<Text, DB> for Product
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(Product::try_from(s)?)
	}
}

impl ToSql<Text, diesel::pg::Pg> for Product
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
		for product in Product::ALL {
			assert_eq!(product.to_string().parse::<Product>().unwrap(), *product);
		}
	}

	#[test]
	fn default_kind_is_one_the_product_defines() {
		for product in Product::ALL {
			assert!(product.defines_kind(product.default_kind()));
		}
	}

	#[test]
	fn only_tamanu_is_graded_against_a_release_train() {
		assert!(Product::Tamanu.tracks_versions());
		assert!(!Product::Canopy.tracks_versions());
		assert!(!Product::Senaite.tracks_versions());
	}

	#[test]
	fn canopy_has_a_version_even_though_it_is_ungraded() {
		assert!(Product::Canopy.has_versions());
		assert!(Product::Tamanu.has_versions());
		assert!(!Product::Senaite.has_versions());
	}

	/// The URL a version resolves to has to match what tamanu's dbt docs
	/// deploy actually publishes, since that upload is the only thing
	/// putting a manifest there.
	#[test]
	fn tamanu_manifest_resolves_to_the_published_location() {
		let manifest = Product::Tamanu.caps().redaction.unwrap();
		assert_eq!(
			manifest.url_for(&node_semver::Version::parse("2.41.3").unwrap()),
			"https://docs.data.bes.au/tamanu/v2.41.3/manifest.json"
		);
	}

	#[test]
	fn only_tamanu_can_be_redacted() {
		assert!(Product::Tamanu.caps().redaction.is_some());
		assert!(Product::Canopy.caps().redaction.is_none());
		assert!(Product::Senaite.caps().redaction.is_none());
	}
}
