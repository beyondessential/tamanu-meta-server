#[cfg(feature = "ssr")]
use diesel::{
	backend::Backend,
	deserialize::{self, FromSql},
	expression::AsExpression,
	serialize::{self, Output, ToSql},
	sql_types::{Array, Float8},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ssr", derive(AsExpression))]
#[cfg_attr(feature = "ssr", diesel(sql_type = Array<Float8>))]
pub struct GeoPoint {
	pub lat: f64,
	pub lon: f64,
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("invalid geo point database type (must be an array of two floats)")]
pub struct InvalidGeoPointDatabaseTypeError;

#[cfg(feature = "ssr")]
impl<DB> FromSql<Array<Float8>, DB> for GeoPoint
where
	DB: Backend,
	Vec<f64>: FromSql<Array<Float8>, DB>,
{
	fn from_sql(bytes: DB::RawValue<'_>) -> deserialize::Result<Self> {
		let arr = Vec::<f64>::from_sql(bytes)?;
		if let [lat, lon] = arr.as_slice() {
			Ok(GeoPoint {
				lat: *lat,
				lon: *lon,
			})
		} else {
			Err(Box::new(InvalidGeoPointDatabaseTypeError))
		}
	}
}

#[cfg(feature = "ssr")]
impl ToSql<Array<Float8>, diesel::pg::Pg> for GeoPoint
where
	Vec<f64>: ToSql<Array<Float8>, diesel::pg::Pg>,
{
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, diesel::pg::Pg>) -> serialize::Result {
		let v = vec![self.lat, self.lon];
		<Vec<f64> as ToSql<Array<Float8>, diesel::pg::Pg>>::to_sql(&v, &mut out.reborrow())
	}
}
