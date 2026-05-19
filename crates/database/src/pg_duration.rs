use diesel::{
	data_types::PgInterval,
	deserialize::{self, FromSqlRow},
	expression::AsExpression,
	pg::Pg,
	serialize,
	sql_types::Interval,
};
use jiff::SignedDuration;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const DAYS_PER_MONTH: i64 = 30;
const HOURS_PER_DAY: i64 = 24;
const SECONDS_PER_DAY: i128 = 60 * 60 * 24;
const MICROSECONDS_PER_SECOND: i128 = 1_000_000;

/// A PostgreSQL `INTERVAL` mapped to a `jiff::SignedDuration`.
///
/// The wire format (serde) is whole seconds as an `i64` — convenient for
/// the React frontend, which only ever talks in seconds/minutes. Sub-second
/// precision in the column round-trips through the DB (via the inner
/// `SignedDuration`) but is truncated on the wire.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, AsExpression, FromSqlRow)]
#[diesel(sql_type = Interval)]
pub struct PgDuration(pub SignedDuration);

impl Serialize for PgDuration {
	fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
		s.serialize_i64(self.0.as_secs())
	}
}

impl<'de> Deserialize<'de> for PgDuration {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		i64::deserialize(d).map(|s| PgDuration(SignedDuration::from_secs(s)))
	}
}

impl serialize::ToSql<Interval, Pg> for PgDuration {
	fn to_sql<'b>(&'b self, out: &mut serialize::Output<'b, '_, Pg>) -> serialize::Result {
		let microseconds = (self.0.as_micros() % (MICROSECONDS_PER_SECOND * SECONDS_PER_DAY))
			.try_into()
			.unwrap_or(i64::MAX);

		let days: i32 = self
			.0
			.as_hours()
			.saturating_div(24)
			.try_into()
			.unwrap_or(i32::MAX);

		// We don't use months here, because in PostgreSQL
		// `timestamp - timestamp` returns interval where
		// every delta is contained in days and microseconds, and 0 months.
		// https://www.postgresql.org/docs/current/functions-datetime.html
		let interval = PgInterval {
			microseconds,
			days,
			months: 0,
		};
		<PgInterval as serialize::ToSql<Interval, Pg>>::to_sql(&interval, &mut out.reborrow())
	}
}

impl deserialize::FromSql<Interval, Pg> for PgDuration {
	fn from_sql(bytes: diesel::pg::PgValue<'_>) -> deserialize::Result<Self> {
		let interval: PgInterval = deserialize::FromSql::<Interval, Pg>::from_sql(bytes)?;
		// We use 1 month = 30 days and 1 day = 24 hours, as postgres
		// use those ratios as default when explicitly converted.
		// For reference, please read `justify_interval` from this page.
		// https://www.postgresql.org/docs/current/functions-datetime.html
		let hours =
			((interval.months as i64) * DAYS_PER_MONTH + (interval.days as i64)) * HOURS_PER_DAY;
		Ok(Self(
			SignedDuration::from_hours(hours) + SignedDuration::from_micros(interval.microseconds),
		))
	}
}
