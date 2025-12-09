use commons_errors::{AppError, Result};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use node_semver::Version as SemverVersion;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::versions::Version;

#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Associations)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::artifacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Artifact {
	pub id: Uuid,
	pub version_id: Option<Uuid>,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
	pub device_id: Option<Uuid>,
	pub version_range_pattern: Option<String>,
}

#[derive(Debug, Deserialize, Insertable)]
#[diesel(belongs_to(Version))]
#[diesel(table_name = crate::schema::artifacts)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewArtifact {
	pub version_id: Option<Uuid>,
	pub artifact_type: String,
	pub platform: String,
	pub download_url: String,
	pub device_id: Option<Uuid>,
	pub version_range_pattern: Option<String>,
}

impl Artifact {
	pub async fn get_for_version(
		db: &mut AsyncPgConnection,
		target_version_id: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::artifacts::*;

		// First, get the version from the database to extract semver
		let version = crate::versions::Version::get_by_id(db, target_version_id).await?;
		let semver = SemverVersion::parse(&format!(
			"{}.{}.{}",
			version.major, version.minor, version.patch
		))?;

		// Query all artifacts (both exact match and range-based)
		let mut artifacts: Vec<Self> = table
			.select(Self::as_select())
			.filter(
				version_id
					.eq(Some(target_version_id))
					.or(version_range_pattern.is_not_null()),
			)
			.order_by(artifact_type.asc())
			.then_order_by(platform.asc())
			.load(db)
			.await
			.map_err(AppError::from)?;

		// Filter out range artifacts that don't match the version
		artifacts.retain(|artifact| {
			if artifact.version_id == Some(target_version_id) {
				// Exact match, always keep
				true
			} else if let Some(pattern) = &artifact.version_range_pattern {
				// Range match, check if version satisfies the pattern
				match node_semver::Range::parse(pattern) {
					Ok(range) => range.satisfies(&semver),
					Err(_) => false, // Invalid pattern, skip this artifact
				}
			} else {
				// Should not happen due to DB constraint, but be safe
				false
			}
		});

		// Sort by specificity to handle conflicts
		Self::sort_by_specificity(&mut artifacts);

		// Remove duplicates by platform+artifact_type, keeping the most specific one
		artifacts.dedup_by_key(|a| (a.artifact_type.clone(), a.platform.clone()));

		Ok(artifacts)
	}

	/// Sort artifacts by specificity, with most specific first.
	/// Priority:
	/// 1. Exact version matches (version_id set)
	/// 2. More specific ranges (range that allows_all of other matching ranges)
	fn sort_by_specificity(artifacts: &mut [Self]) {
		artifacts.sort_by(|a, b| {
			// Exact match always wins
			let a_is_exact = a.version_id.is_some();
			let b_is_exact = b.version_id.is_some();

			if a_is_exact && !b_is_exact {
				return std::cmp::Ordering::Less; // a is more specific
			}
			if !a_is_exact && b_is_exact {
				return std::cmp::Ordering::Greater; // b is more specific
			}

			// Both exact or both range: compare range specificity
			if !a_is_exact {
				if let (Some(pattern_a), Some(pattern_b)) =
					(&a.version_range_pattern, &b.version_range_pattern)
				{
					if let (Ok(range_a), Ok(range_b)) = (
						node_semver::Range::parse(pattern_a),
						node_semver::Range::parse(pattern_b),
					) {
						// If range_a allows_all of range_b, then range_b is more specific
						if range_a.allows_all(&range_b) && !range_b.allows_all(&range_a) {
							return std::cmp::Ordering::Greater; // b is more specific
						}
						// If range_b allows_all of range_a, then range_a is more specific
						if range_b.allows_all(&range_a) && !range_a.allows_all(&range_b) {
							return std::cmp::Ordering::Less; // a is more specific
						}
					}
				}
			}

			// Can't determine specificity, maintain order
			std::cmp::Ordering::Equal
		});
	}

	pub async fn update(
		db: &mut AsyncPgConnection,
		artifact_id: Uuid,
		new_type: String,
		new_platform: String,
		new_url: String,
	) -> Result<()> {
		use crate::schema::artifacts::dsl::*;

		diesel::update(artifacts.filter(id.eq(artifact_id)))
			.set((
				artifact_type.eq(new_type),
				platform.eq(new_platform),
				download_url.eq(new_url),
			))
			.execute(db)
			.await?;

		Ok(())
	}

	pub async fn create(
		db: &mut AsyncPgConnection,
		ver_id: Uuid,
		art_type: String,
		plat: String,
		url: String,
	) -> Result<Self> {
		use crate::schema::artifacts::dsl::*;

		let new_artifact = NewArtifact {
			version_id: Some(ver_id),
			artifact_type: art_type,
			platform: plat,
			download_url: url,
			device_id: None,
			version_range_pattern: None,
		};

		diesel::insert_into(artifacts)
			.values(new_artifact)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn delete(db: &mut AsyncPgConnection, artifact_id: Uuid) -> Result<()> {
		use crate::schema::artifacts::dsl::*;

		diesel::delete(artifacts.filter(id.eq(artifact_id)))
			.execute(db)
			.await?;

		Ok(())
	}
}
