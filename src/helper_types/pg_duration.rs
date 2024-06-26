use chrono::Duration;
use diesel::{
	data_types::PgInterval, deserialize, expression::AsExpression, pg::Pg, serialize,
	sql_types::Interval,
};
use rocket::serde::Serialize;

const DAYS_PER_MONTH: i32 = 30;
const SECONDS_PER_DAY: i64 = 60 * 60 * 24;
const MICROSECONDS_PER_SECOND: i64 = 1_000_000;

#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, AsExpression)]
#[diesel(sql_type = Interval)]
pub struct PgDuration(pub chrono::Duration);

impl Serialize for PgDuration {
	fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
	where
		S: rocket::serde::Serializer,
	{
		self.0.num_seconds().serialize(serializer)
	}
}

impl serialize::ToSql<Interval, Pg> for PgDuration {
	fn to_sql<'b>(&'b self, out: &mut serialize::Output<'b, '_, Pg>) -> serialize::Result {
		let microseconds: i64 = if let Some(v) = self.0.num_microseconds() {
			v % (MICROSECONDS_PER_SECOND * SECONDS_PER_DAY)
		} else {
			return Err("Failed to create microseconds by overflow".into());
		};
		let days: i32 = self
			.0
			.num_days()
			.try_into()
			.expect("Failed to get i32 days from i64");
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
		let days = interval.months * DAYS_PER_MONTH + interval.days;
		Ok(Self(
			Duration::days(days as i64) + Duration::microseconds(interval.microseconds),
		))
	}
}
