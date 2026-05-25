use std::collections::BTreeMap;

use diesel::{
	deserialize::{self, FromSql, FromSqlRow},
	expression::AsExpression,
	pg::Pg,
	serialize::{self, IsNull, Output, ToSql},
	sql_types::Jsonb,
};
use serde::{Deserialize, Serialize};

/// Free-form string→string key/value tags attached to a server or server group.
///
/// Wire and DB representation is a JSON object whose values are all strings.
/// Non-string values from the DB are rejected at decode time. Stored in
/// PostgreSQL as `JSONB`; the column has a `jsonb_typeof(...) = 'object'`
/// check constraint, so non-object JSON never reaches the application.
#[derive(
	Debug, Clone, Default, Eq, PartialEq, Serialize, Deserialize, AsExpression, FromSqlRow,
)]
#[diesel(sql_type = Jsonb)]
#[serde(transparent)]
pub struct TagMap(pub BTreeMap<String, String>);

impl TagMap {
	pub fn new() -> Self {
		Self(BTreeMap::new())
	}

	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}

	/// Overlay `self` onto `base` (server overlays group): every key in `self`
	/// wins; keys present only in `base` carry through. `self` is not
	/// modified — the merge returns a fresh map.
	pub fn merged_with(&self, base: &TagMap) -> TagMap {
		let mut out = base.0.clone();
		out.extend(self.0.iter().map(|(k, v)| (k.clone(), v.clone())));
		TagMap(out)
	}
}

impl utoipa::PartialSchema for TagMap {
	fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
		use utoipa::openapi::schema::{AdditionalProperties, Object, SchemaType, Type};

		let mut object = Object::with_type(SchemaType::Type(Type::Object));
		object.additional_properties = Some(Box::new(AdditionalProperties::RefOr(
			utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(Object::with_type(
				SchemaType::Type(Type::String),
			))),
		)));
		utoipa::openapi::RefOr::T(utoipa::openapi::schema::Schema::Object(object))
	}
}

impl utoipa::ToSchema for TagMap {}

impl ToSql<Jsonb, Pg> for TagMap {
	fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
		// Match the wire format: { key: value, ... } as a JSON object. The
		// leading `1` byte is JSONB's version prefix in the PG binary
		// protocol (current spec).
		out.write_all(&[1])?;
		serde_json::to_writer(out, &self.0)?;
		Ok(IsNull::No)
	}
}

impl FromSql<Jsonb, Pg> for TagMap {
	fn from_sql(bytes: diesel::pg::PgValue<'_>) -> deserialize::Result<Self> {
		let raw = bytes.as_bytes();
		// Skip JSONB version prefix.
		let body = raw.strip_prefix(&[1]).unwrap_or(raw);
		let value: serde_json::Value = serde_json::from_slice(body)?;
		let map = value
			.as_object()
			.ok_or("tags column was not a JSON object")?;
		let mut out = BTreeMap::new();
		for (k, v) in map {
			let s = v.as_str().ok_or_else(|| {
				format!("tags[{}] was not a string ({} found)", k, type_name_of(v))
			})?;
			out.insert(k.clone(), s.to_string());
		}
		Ok(TagMap(out))
	}
}

fn type_name_of(v: &serde_json::Value) -> &'static str {
	match v {
		serde_json::Value::Null => "null",
		serde_json::Value::Bool(_) => "bool",
		serde_json::Value::Number(_) => "number",
		serde_json::Value::String(_) => "string",
		serde_json::Value::Array(_) => "array",
		serde_json::Value::Object(_) => "object",
	}
}

use std::io::Write as _;
