//! Shared vocabulary for the backup-credentials system.
//!
//! These enums are stored as `TEXT` in Postgres and flow through
//! public-server, the `jobs` crate, and the private-web wire types (via the
//! generated `api-types.ts`), so they live here in `commons-types` rather than
//! being re-declared per component. The closed sets (`BackupPurpose`,
//! `RunOutcome`, `MaintenanceKind`, `BackupConfigStatus`) mirror the
//! `Severity`/`ServerKind` pattern and back DB `CHECK (… IN …)` constraints.
//! `BackupType` is deliberately open — bestool registers arbitrary type names
//! and unknown ones land in `Custom`.

use std::{fmt::Display, str::FromStr};

use diesel::{
	backend::Backend,
	deserialize::{self, FromSql, FromSqlRow},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// Generate a closed string-valued enum stored as Postgres `TEXT`, with the
/// `Display`/`FromStr`/`FromSql`/`ToSql` plumbing the `Severity` pattern uses.
/// The string literal per variant is the single source of truth for the wire
/// form, the `Display` form, and the DB value (which the `CHECK` constraint
/// must match).
macro_rules! text_enum {
	(
		$(#[$meta:meta])*
		$vis:vis enum $name:ident {
			$( $(#[doc = $doc:literal])* $variant:ident = $str:literal ),+ $(,)?
		}
		default = $default:ident;
		error $err:ident = $errmsg:literal;
	) => {
		$(#[$meta])*
		#[derive(
			Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, AsExpression, FromSqlRow,
			utoipa::ToSchema,
		)]
		#[diesel(sql_type = Text)]
		$vis enum $name {
			// `serde(rename)` sets the wire string; `schema(rename)` mirrors it
			// into the utoipa-generated OpenAPI schema, since utoipa v5 doesn't
			// pick up per-variant serde renames on its own (it only honours a
			// container-level `rename_all`). Without the schema rename the
			// generated TS union would carry the PascalCase variant names while
			// the actual JSON is the lowercase string — a wire/type mismatch.
			$( $(#[doc = $doc])* #[serde(rename = $str)] #[schema(rename = $str)] $variant ),+
		}

		#[derive(Debug, Clone, Copy, thiserror::Error)]
		#[error($errmsg)]
		$vis struct $err;

		impl Default for $name {
			fn default() -> Self { Self::$default }
		}

		impl Display for $name {
			fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
				let s = match self { $( Self::$variant => $str ),+ };
				write!(f, "{s}")
			}
		}

		impl FromStr for $name {
			type Err = $err;
			fn from_str(s: &str) -> Result<Self, Self::Err> {
				match s { $( $str => Ok(Self::$variant), )+ _ => Err($err) }
			}
		}

		impl TryFrom<String> for $name {
			type Error = $err;
			fn try_from(value: String) -> Result<Self, Self::Error> { value.parse() }
		}

		impl From<$name> for String {
			fn from(v: $name) -> Self { v.to_string() }
		}

		impl<DB> FromSql<Text, DB> for $name
		where
			DB: Backend,
			String: FromSql<Text, DB>,
		{
			fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
				Ok(Self::try_from(String::from_sql(bytes)?)?)
			}
		}

		impl ToSql<Text, diesel::pg::Pg> for $name
		where
			String: ToSql<Text, diesel::pg::Pg>,
		{
			fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
				let v = String::from(*self);
				<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
			}
		}
	};
}

text_enum! {
	/// Why a credential was issued / a run executed. A real capability gate
	/// on the issued S3 creds, not just audit metadata: `Backup` grants
	/// write-without-delete, `Restore` grants read-only.
	pub enum BackupPurpose {
		Backup = "backup",
		Restore = "restore",
	}
	default = Backup;
	error BackupPurposeFromStringError = "invalid backup purpose; expected one of: backup, restore";
}

text_enum! {
	/// Outcome of a reported backup/restore run.
	pub enum RunOutcome {
		Success = "success",
		Failure = "failure",
	}
	default = Success;
	error RunOutcomeFromStringError = "invalid run outcome; expected one of: success, failure";
}

text_enum! {
	/// Which kopia maintenance cycle a Canopy maintenance Job ran.
	pub enum MaintenanceKind {
		Quick = "quick",
		Full = "full",
	}
	default = Quick;
	error MaintenanceKindFromStringError = "invalid maintenance kind; expected one of: quick, full";
}

text_enum! {
	/// How a group's repo passphrase is sourced. Canopy owns every passphrase
	/// Secret either way. `FromBirth` means Canopy generates the passphrase, then
	/// the operator escrows it via the reveal-once flow (`escrow_pending`).
	/// `Passphrase` means the operator supplies the passphrase (for a fresh repo
	/// or to connect an existing one); Canopy stores it and skips escrow (straight
	/// to `Ready`).
	pub enum BackupRepoMode {
		FromBirth = "from_birth",
		Passphrase = "passphrase",
	}
	default = FromBirth;
	error BackupRepoModeFromStringError = "invalid backup repo mode; expected one of: from_birth, passphrase";
}

text_enum! {
	/// Lifecycle state of a group's backup repo. Backups stay dormant (the
	/// endpoints 412/409) until `Ready`.
	pub enum BackupConfigStatus {
		/// Init Job running.
		Provisioning = "provisioning",
		/// Repo created, awaiting the operator's Bitwarden-escrow ack.
		EscrowPending = "escrow_pending",
		/// Authorized: config set + repo created + escrow acknowledged.
		Ready = "ready",
	}
	default = Provisioning;
	error BackupConfigStatusFromStringError = "invalid backup config status; expected one of: provisioning, escrow_pending, ready";
}

/// A named, client-side backup procedure that ends in a type-tagged kopia
/// snapshot (e.g. `tamanu-postgres`). Open by design: bestool advertises
/// whatever types its server can run, and Canopy is type-agnostic for
/// execution, so any unrecognised name is preserved verbatim in `Custom`
/// rather than rejected. Stored as `TEXT`; serializes as a plain string (no
/// DB `CHECK`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, AsExpression, FromSqlRow)]
#[diesel(sql_type = Text)]
pub enum BackupType {
	/// The first well-known type — a quiesced Postgres snapshot (also what
	/// PGRO restores).
	TamanuPostgres,
	/// Any other type name, preserved as advertised.
	Custom(String),
}

impl BackupType {
	const TAMANU_POSTGRES: &'static str = "tamanu-postgres";

	/// The wire/DB string for this type.
	pub fn as_str(&self) -> &str {
		match self {
			Self::TamanuPostgres => Self::TAMANU_POSTGRES,
			Self::Custom(s) => s,
		}
	}
}

impl Display for BackupType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(self.as_str())
	}
}

impl FromStr for BackupType {
	type Err = std::convert::Infallible;
	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Ok(match s {
			Self::TAMANU_POSTGRES => Self::TamanuPostgres,
			other => Self::Custom(other.to_owned()),
		})
	}
}

impl From<&str> for BackupType {
	fn from(s: &str) -> Self {
		// FromStr is infallible.
		s.parse().unwrap()
	}
}

impl From<String> for BackupType {
	fn from(s: String) -> Self {
		match s.as_str() {
			Self::TAMANU_POSTGRES => Self::TamanuPostgres,
			_ => Self::Custom(s),
		}
	}
}

impl From<BackupType> for String {
	fn from(v: BackupType) -> Self {
		match v {
			BackupType::TamanuPostgres => BackupType::TAMANU_POSTGRES.to_owned(),
			BackupType::Custom(s) => s,
		}
	}
}

impl Serialize for BackupType {
	fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
		s.serialize_str(self.as_str())
	}
}

impl<'de> Deserialize<'de> for BackupType {
	fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		Ok(Self::from(String::deserialize(d)?))
	}
}

impl<DB> FromSql<Text, DB> for BackupType
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		Ok(Self::from(String::from_sql(bytes)?))
	}
}

impl ToSql<Text, diesel::pg::Pg> for BackupType
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		<str as ToSql<Text, diesel::pg::Pg>>::to_sql(self.as_str(), &mut out.reborrow())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn closed_enum_roundtrips_match_db_strings() {
		assert_eq!(BackupPurpose::Backup.to_string(), "backup");
		assert_eq!(
			"restore".parse::<BackupPurpose>().unwrap(),
			BackupPurpose::Restore
		);
		assert_eq!(
			BackupConfigStatus::EscrowPending.to_string(),
			"escrow_pending"
		);
		assert!("nope".parse::<RunOutcome>().is_err());
	}

	#[test]
	fn closed_enum_serde_is_the_db_string() {
		assert_eq!(
			serde_json::to_string(&MaintenanceKind::Full).unwrap(),
			"\"full\""
		);
		assert_eq!(
			serde_json::from_str::<BackupConfigStatus>("\"provisioning\"").unwrap(),
			BackupConfigStatus::Provisioning,
		);
	}

	#[test]
	fn backup_type_known_and_custom() {
		assert_eq!(
			BackupType::from("tamanu-postgres"),
			BackupType::TamanuPostgres
		);
		assert_eq!(
			BackupType::from("weird"),
			BackupType::Custom("weird".into())
		);
		assert_eq!(BackupType::TamanuPostgres.to_string(), "tamanu-postgres");
		// Round-trips through the wire as a plain string.
		assert_eq!(
			serde_json::to_string(&BackupType::TamanuPostgres).unwrap(),
			"\"tamanu-postgres\""
		);
		assert_eq!(
			serde_json::from_str::<BackupType>("\"custom-x\"").unwrap(),
			BackupType::Custom("custom-x".into()),
		);
	}
}
