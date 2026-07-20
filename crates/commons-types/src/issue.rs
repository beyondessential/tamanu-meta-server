use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// Reason an issue or incident was resolved by an operator.
///
/// Stored as text in Postgres, validated as this enum at the API layer.
/// Categories follow common operational practice (PagerDuty/Sentry/Opsgenie):
///
/// - `Fixed` — the underlying problem was addressed.
/// - `WontFix` — won't fix; not worth doing, or not actually a problem.
/// - `Expected` — known/expected behaviour (e.g. planned maintenance window).
/// - `Duplicate` — a duplicate of another issue.
/// - `Flapping` — too noisy; suppressed rather than fixed (often paired with
///   a snooze).
/// - `Decommissioned` — the check itself was retired fleet-wide; its states
///   are resolved as a side effect, not by an operator addressing them.
#[derive(
	Debug,
	Clone,
	Copy,
	Default,
	PartialEq,
	Eq,
	Serialize,
	Deserialize,
	AsExpression,
	utoipa::ToSchema,
)]
#[diesel(sql_type = Text)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedReason {
	#[default]
	Fixed,
	WontFix,
	Expected,
	Duplicate,
	Flapping,
	Decommissioned,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error(
	"invalid resolved reason; expected one of: fixed, wont_fix, expected, duplicate, flapping, decommissioned"
)]
pub struct ResolvedReasonFromStringError;

impl std::str::FromStr for ResolvedReason {
	type Err = ResolvedReasonFromStringError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_ascii_lowercase().as_ref() {
			"fixed" => Ok(Self::Fixed),
			"wont_fix" | "wontfix" | "won't_fix" => Ok(Self::WontFix),
			"expected" => Ok(Self::Expected),
			"duplicate" | "dup" => Ok(Self::Duplicate),
			"flapping" | "flap" => Ok(Self::Flapping),
			"decommissioned" | "decommission" => Ok(Self::Decommissioned),
			_ => Err(ResolvedReasonFromStringError),
		}
	}
}

impl TryFrom<String> for ResolvedReason {
	type Error = ResolvedReasonFromStringError;

	fn try_from(value: String) -> Result<Self, ResolvedReasonFromStringError> {
		value.parse()
	}
}

impl std::fmt::Display for ResolvedReason {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let s = match self {
			Self::Fixed => "fixed",
			Self::WontFix => "wont_fix",
			Self::Expected => "expected",
			Self::Duplicate => "duplicate",
			Self::Flapping => "flapping",
			Self::Decommissioned => "decommissioned",
		};
		write!(f, "{}", s)
	}
}

impl From<ResolvedReason> for String {
	fn from(s: ResolvedReason) -> Self {
		s.to_string()
	}
}

impl<DB> FromSql<Text, DB> for ResolvedReason
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(ResolvedReason::try_from(s)?)
	}
}

impl ToSql<Text, diesel::pg::Pg> for ResolvedReason
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
