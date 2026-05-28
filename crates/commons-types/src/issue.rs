use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::Text,
};
use serde::{Deserialize, Serialize};

/// RFC 5424 syslog severities.
///
/// Stored as text in Postgres; validated as this enum at the API layer.
/// Default is `Error` (incidents only open at severity ≥ Error, so the
/// default is intentionally above the floor — most devices that bother
/// pushing an event mean it).
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
#[serde(rename_all = "lowercase")]
pub enum Severity {
	Emergency,
	Alert,
	Critical,
	#[default]
	Error,
	Warning,
	Notice,
	Info,
	Debug,
}

impl Severity {
	/// Severities that may open an incident on their own and may keep one
	/// open. Low-severity issues (warning and below) join an already-open
	/// incident for context but don't hold it open — see
	/// `database::issues::re_evaluate_incident_membership`.
	pub const OPENS_INCIDENT: &'static [Severity] = &[
		Self::Emergency,
		Self::Alert,
		Self::Critical,
		Self::Error,
	];

	/// Issues at or above this severity open incidents.
	pub fn opens_incident(self) -> bool {
		Self::OPENS_INCIDENT.contains(&self)
	}
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error(
	"invalid severity; expected one of: emergency, alert, critical, error, warning, notice, info, debug"
)]
pub struct SeverityFromStringError;

impl std::str::FromStr for Severity {
	type Err = SeverityFromStringError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.to_ascii_lowercase().as_ref() {
			"emergency" | "emerg" | "panic" => Ok(Self::Emergency),
			"alert" => Ok(Self::Alert),
			"critical" | "crit" => Ok(Self::Critical),
			"error" | "err" => Ok(Self::Error),
			"warning" | "warn" => Ok(Self::Warning),
			"notice" => Ok(Self::Notice),
			"info" | "informational" => Ok(Self::Info),
			"debug" => Ok(Self::Debug),
			_ => Err(SeverityFromStringError),
		}
	}
}

impl TryFrom<String> for Severity {
	type Error = SeverityFromStringError;

	fn try_from(value: String) -> Result<Self, SeverityFromStringError> {
		value.parse()
	}
}

impl std::fmt::Display for Severity {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let s = match self {
			Self::Emergency => "emergency",
			Self::Alert => "alert",
			Self::Critical => "critical",
			Self::Error => "error",
			Self::Warning => "warning",
			Self::Notice => "notice",
			Self::Info => "info",
			Self::Debug => "debug",
		};
		write!(f, "{}", s)
	}
}

impl From<Severity> for String {
	fn from(s: Severity) -> Self {
		s.to_string()
	}
}

impl<DB> FromSql<Text, DB> for Severity
where
	DB: Backend,
	String: FromSql<Text, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let s = String::from_sql(bytes)?;
		Ok(Severity::try_from(s)?)
	}
}

impl ToSql<Text, diesel::pg::Pg> for Severity
where
	String: ToSql<Text, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = String::from(*self);
		<String as ToSql<Text, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}

/// Reason an issue or incident was resolved by a human.
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
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid resolved reason; expected one of: fixed, wont_fix, expected, duplicate, flapping")]
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
