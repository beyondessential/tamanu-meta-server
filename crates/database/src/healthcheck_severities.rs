//! Operator-owned catalog of healthcheck names → the severity to file
//! their failures at. See `docs/plans/healthcheck-severity-catalog.md`.
//!
//! Ingestion (in the public-server status handler) calls
//! [`HealthcheckSeverity::upsert_default`] for every check name seen on
//! a push, then [`HealthcheckSeverity::severity_for`] when filing a
//! failing per-check issue. Operators read and edit the catalog via the
//! private-server `/api/healthchecks` endpoints.

use commons_errors::{AppError, Result};
use commons_types::{issue::Severity, status::CheckResult};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::healthcheck_severities)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct HealthcheckSeverity {
	pub check_name: String,
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub severity: Severity,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub first_seen: Timestamp,
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub reviewed_at: Option<Timestamp>,
	pub reviewed_by: Option<String>,
	pub notes: Option<String>,
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// JsonLogic `if`-ladder evaluating to a Severity string. NULL ⇒
	/// no conditional rules; `severity` is used directly. Format and
	/// constraints documented on [`IfLadder`].
	#[schema(value_type = Option<serde_json::Value>)]
	pub rules: Option<JsonValue>,
}

impl HealthcheckSeverity {
	/// Insert a row for `check_name` with default values (severity =
	/// warning, reviewed_at = NULL) if and only if no row exists yet.
	/// Idempotent: safe to call on every status push for every check
	/// seen, including healthy ones. Concurrent pushes are serialised
	/// by Postgres via `ON CONFLICT DO NOTHING`.
	pub async fn upsert_default(db: &mut AsyncPgConnection, check_name: &str) -> Result<()> {
		use crate::schema::healthcheck_severities::dsl;
		diesel::insert_into(dsl::healthcheck_severities)
			.values(dsl::check_name.eq(check_name))
			.on_conflict(dsl::check_name)
			.do_nothing()
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Look up the effective severity for a `check_name` reporting
	/// `result` (warning or failed — the only kinds that file at the
	/// catalog severity), given the supplied evaluation context. If
	/// the row has a `rules` ladder it's evaluated against the context
	/// first; the first matching branch's severity wins. Otherwise (no
	/// ladder, no matching branch, or malformed JSON) the fallback
	/// depends on the result kind: warning-result checks land at fixed
	/// [`Severity::Warning`]; failed checks use the row's base
	/// `severity` column. The catalog column is thus "the severity of
	/// this check's failures" — warnings only deviate from Warning via
	/// an explicit rule (which can condition on `check.result`).
	///
	/// Falls back to `Severity::Warning` if no row exists yet — in
	/// practice the status handler upserts before reading, so this
	/// branch only covers the genuine race / programmer-error case.
	pub async fn severity_for(
		db: &mut AsyncPgConnection,
		check_name: &str,
		result: CheckResult,
		ctx: &EvaluationContext<'_>,
	) -> Result<Severity> {
		use crate::schema::healthcheck_severities::dsl;
		let row: Option<(String, Option<JsonValue>)> = dsl::healthcheck_severities
			.select((dsl::severity, dsl::rules))
			.filter(dsl::check_name.eq(check_name))
			.first(db)
			.await
			.optional()?;
		let Some((sev_str, rules_json)) = row else {
			return Ok(Severity::Warning);
		};
		let base = sev_str.parse().unwrap_or(Severity::Warning);
		if let Some(rules) = rules_json {
			match serde_json::from_value::<IfLadder>(rules) {
				Ok(ladder) => {
					if let Some(s) = ladder.evaluate(ctx) {
						return Ok(s);
					}
				}
				Err(err) => {
					tracing::warn!(
						check_name,
						?err,
						"failed to parse healthcheck severity rules; falling back to base"
					);
				}
			}
		}
		Ok(match result {
			CheckResult::Warning => Severity::Warning,
			_ => base,
		})
	}

	/// Replace the conditional-rules ladder for a check (or clear it
	/// with `None`). Stamps `reviewed_at` / `reviewed_by`, so editing
	/// rules also counts as a review for the catalog row.
	pub async fn update_rules(
		db: &mut AsyncPgConnection,
		check_name: &str,
		rules: Option<&IfLadder>,
		by: &str,
	) -> Result<Self> {
		use crate::schema::healthcheck_severities::dsl;
		let now = Timestamp::now();
		let rules_json: Option<JsonValue> =
			rules.map(|l| serde_json::to_value(l).expect("IfLadder always serialises"));
		diesel::update(dsl::healthcheck_severities.filter(dsl::check_name.eq(check_name)))
			.set((
				dsl::rules.eq(rules_json),
				dsl::reviewed_at.eq(jiff_diesel::Timestamp::from(now)),
				dsl::reviewed_by.eq(by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::healthcheck_severities::dsl;
		dsl::healthcheck_severities
			.select(Self::as_select())
			.order(dsl::check_name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Update the severity (and optionally notes) for a check, stamping
	/// `reviewed_at = NOW()` and `reviewed_by = by`. Even a no-op save
	/// (same severity) marks the row reviewed — operators can ack
	/// a check without changing it.
	pub async fn update(
		db: &mut AsyncPgConnection,
		check_name: &str,
		severity: Severity,
		notes: Option<&str>,
		by: &str,
	) -> Result<Self> {
		use crate::schema::healthcheck_severities::dsl;
		let now = Timestamp::now();
		diesel::update(dsl::healthcheck_severities.filter(dsl::check_name.eq(check_name)))
			.set((
				dsl::severity.eq(severity),
				dsl::notes.eq(notes),
				dsl::reviewed_at.eq(jiff_diesel::Timestamp::from(now)),
				dsl::reviewed_by.eq(by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}
}

// ── Rule model: JsonLogic-encoded if-ladder ────────────────────────────────

/// A JsonLogic `if`-ladder evaluating to a Severity string.
///
/// Wire shape: `{"if": [c1, s1, c2, s2, …, cN, sN]}` — even-length
/// argument list, every odd-index entry is a [`Condition`], every
/// even-index entry is a Severity literal. No trailing else: when no
/// branch matches, [`Self::evaluate`] returns `None` and the calling
/// `severity_for` falls through to the row's `severity` column.
///
/// Empty ladders are forbidden by the deserialiser; the API layer
/// normalises them to `None` (clearing the `rules` column) before
/// hitting the database.
#[derive(Clone, Debug, PartialEq)]
pub struct IfLadder {
	pub branches: Vec<(Condition, Severity)>,
}

/// One predicate inside an [`IfLadder`] branch. Maps 1:1 with a single
/// JsonLogic operator; composition (`and`/`or`/`!`, nested `if`) is
/// rejected at deserialise time.
#[derive(Clone, Debug, PartialEq)]
pub enum Condition {
	Eq(Var, JsonValue),
	Neq(Var, JsonValue),
	Lt(Var, JsonValue),
	Lte(Var, JsonValue),
	Gt(Var, JsonValue),
	Gte(Var, JsonValue),
	/// `value` is a `node_semver::Range`-parseable string.
	InRange(Var, String),
}

/// Dotted path into the [`EvaluationContext`]: `kind.field`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Var {
	pub kind: VarKind,
	pub field: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
	Check,
	Status,
	Tag,
}

/// Inputs available to a rule at evaluation time. Built once per status
/// push and shared across all rule evaluations for that push.
pub struct EvaluationContext<'a> {
	/// Top-level status extras (`statuses.extra`). Bestool sends
	/// `bestoolVersion`, `tamanuVersion`, `uptimeSecs`, etc.
	pub status_extra: &'a serde_json::Map<String, JsonValue>,
	/// The failing check's own fields (the `health[i]` object minus
	/// `check` and `healthy`).
	pub check_extra: &'a serde_json::Map<String, JsonValue>,
	/// Server's resolved tag map (merged server + group). Each value is
	/// already wrapped as `JsonValue::String` for uniform comparison.
	pub tags: &'a HashMap<String, JsonValue>,
}

impl IfLadder {
	/// Returns the first matching branch's severity, or `None` if no
	/// branch matches. The caller falls back to the catalog's base
	/// severity in that case.
	pub fn evaluate(&self, ctx: &EvaluationContext) -> Option<Severity> {
		self.branches
			.iter()
			.find_map(|(c, s)| c.matches(ctx).then_some(*s))
	}
}

impl Var {
	pub fn resolve<'a>(&self, ctx: &'a EvaluationContext<'a>) -> Option<&'a JsonValue> {
		match self.kind {
			VarKind::Check => ctx.check_extra.get(&self.field),
			VarKind::Status => ctx.status_extra.get(&self.field),
			VarKind::Tag => ctx.tags.get(&self.field),
		}
	}
}

impl Condition {
	/// Borrow the variable and right-hand operand without cloning.
	fn var(&self) -> &Var {
		match self {
			Self::Eq(v, _)
			| Self::Neq(v, _)
			| Self::Lt(v, _)
			| Self::Lte(v, _)
			| Self::Gt(v, _)
			| Self::Gte(v, _)
			| Self::InRange(v, _) => v,
		}
	}

	pub fn matches(&self, ctx: &EvaluationContext) -> bool {
		let Some(lhs) = self.var().resolve(ctx) else {
			return false;
		};
		match self {
			Self::Eq(_, rhs) => json_equal(lhs, rhs),
			Self::Neq(_, rhs) => !json_equal(lhs, rhs),
			Self::Lt(_, rhs) => json_compare(lhs, rhs, |a, b| a < b).unwrap_or(false),
			Self::Lte(_, rhs) => json_compare(lhs, rhs, |a, b| a <= b).unwrap_or(false),
			Self::Gt(_, rhs) => json_compare(lhs, rhs, |a, b| a > b).unwrap_or(false),
			Self::Gte(_, rhs) => json_compare(lhs, rhs, |a, b| a >= b).unwrap_or(false),
			Self::InRange(_, range_str) => {
				let Some(lhs_str) = lhs.as_str() else {
					return false;
				};
				let Ok(version) = lhs_str.parse::<node_semver::Version>() else {
					return false;
				};
				let Ok(range) = range_str.parse::<node_semver::Range>() else {
					return false;
				};
				range.satisfies(&version)
			}
		}
	}
}

/// Bestool sometimes sends what look like numeric values as JSON strings
/// (e.g. `"currentSyncTick": "21608625"`). Both `==`/`!=` and the numeric
/// comparisons treat such strings numerically when both sides are
/// numeric-coercible. Strict JSON equality is the first check, so
/// `"abc" == "abc"` still matches; the coercion only kicks in when at
/// least one side is a number.
fn to_f64(v: &JsonValue) -> Option<f64> {
	match v {
		JsonValue::Number(n) => n.as_f64(),
		JsonValue::String(s) => s.parse::<f64>().ok(),
		_ => None,
	}
}

fn json_equal(a: &JsonValue, b: &JsonValue) -> bool {
	if a == b {
		return true;
	}
	if let (Some(an), Some(bn)) = (to_f64(a), to_f64(b)) {
		return an == bn;
	}
	false
}

fn json_compare<F: FnOnce(f64, f64) -> bool>(a: &JsonValue, b: &JsonValue, f: F) -> Option<bool> {
	let an = to_f64(a)?;
	let bn = to_f64(b)?;
	Some(f(an, bn))
}

// ── Serde: typed <-> JsonLogic ─────────────────────────────────────────────

impl std::str::FromStr for Var {
	type Err = String;
	fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
		let (kind_str, field) = s
			.split_once('.')
			.ok_or_else(|| format!("var path '{s}' is missing a '.<field>' segment"))?;
		let kind = match kind_str {
			"check" => VarKind::Check,
			"status" => VarKind::Status,
			"tag" => VarKind::Tag,
			other => {
				return Err(format!(
					"unknown var namespace '{other}' (expected check, status, or tag)"
				));
			}
		};
		if field.is_empty() {
			return Err(format!("var path '{s}' has an empty field name"));
		}
		if !field.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
			return Err(format!(
				"var field '{field}' must be ASCII alphanumeric or underscore"
			));
		}
		Ok(Var {
			kind,
			field: field.to_string(),
		})
	}
}

impl std::fmt::Display for Var {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let kind = match self.kind {
			VarKind::Check => "check",
			VarKind::Status => "status",
			VarKind::Tag => "tag",
		};
		write!(f, "{kind}.{}", self.field)
	}
}

impl Serialize for Condition {
	fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
		let (op, rhs): (&str, JsonValue) = match self {
			Self::Eq(_, v) => ("==", v.clone()),
			Self::Neq(_, v) => ("!=", v.clone()),
			Self::Lt(_, v) => ("<", v.clone()),
			Self::Lte(_, v) => ("<=", v.clone()),
			Self::Gt(_, v) => (">", v.clone()),
			Self::Gte(_, v) => (">=", v.clone()),
			Self::InRange(_, r) => ("in_range", JsonValue::String(r.clone())),
		};
		let var_obj = serde_json::json!({ "var": self.var().to_string() });
		let value = serde_json::json!({ op: [var_obj, rhs] });
		value.serialize(ser)
	}
}

impl<'de> Deserialize<'de> for Condition {
	fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
		let value = JsonValue::deserialize(de)?;
		let obj = value
			.as_object()
			.ok_or_else(|| de::Error::custom("condition must be a JSON object"))?;
		if obj.len() != 1 {
			return Err(de::Error::custom(
				"condition object must have exactly one key (the JsonLogic op)",
			));
		}
		let (op, args) = obj.iter().next().expect("len == 1");
		let args = args
			.as_array()
			.ok_or_else(|| de::Error::custom(format!("'{op}' args must be an array")))?;
		if args.len() != 2 {
			return Err(de::Error::custom(format!(
				"'{op}' must have exactly 2 args (got {})",
				args.len()
			)));
		}
		let var_obj = args[0]
			.as_object()
			.ok_or_else(|| de::Error::custom("first arg must be a {\"var\": ...} object"))?;
		if var_obj.len() != 1 {
			return Err(de::Error::custom(
				"first arg must be {\"var\": \"<dotted>\"} with no extras",
			));
		}
		let var_str = var_obj
			.get("var")
			.and_then(|v| v.as_str())
			.ok_or_else(|| de::Error::custom("first arg's 'var' must be a string"))?;
		let var: Var = var_str.parse().map_err(de::Error::custom)?;
		let rhs = args[1].clone();
		match op.as_str() {
			"==" => Ok(Self::Eq(var, rhs)),
			"!=" => Ok(Self::Neq(var, rhs)),
			"<" => Ok(Self::Lt(var, rhs)),
			"<=" => Ok(Self::Lte(var, rhs)),
			">" => Ok(Self::Gt(var, rhs)),
			">=" => Ok(Self::Gte(var, rhs)),
			"in_range" => {
				let range = rhs
					.as_str()
					.ok_or_else(|| de::Error::custom("'in_range' value must be a string"))?;
				range.parse::<node_semver::Range>().map_err(|e| {
					de::Error::custom(format!("invalid semver range '{range}': {e}"))
				})?;
				Ok(Self::InRange(var, range.to_string()))
			}
			other => Err(de::Error::custom(format!(
				"unsupported op '{other}' (allowed: ==, !=, <, <=, >, >=, in_range)"
			))),
		}
	}
}

impl Serialize for IfLadder {
	fn serialize<S: Serializer>(&self, ser: S) -> std::result::Result<S::Ok, S::Error> {
		let mut args: Vec<JsonValue> = Vec::with_capacity(self.branches.len() * 2);
		for (c, s) in &self.branches {
			args.push(serde_json::to_value(c).map_err(serde::ser::Error::custom)?);
			args.push(JsonValue::String(s.to_string()));
		}
		serde_json::json!({ "if": args }).serialize(ser)
	}
}

impl<'de> Deserialize<'de> for IfLadder {
	fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
		let value = JsonValue::deserialize(de)?;
		let obj = value
			.as_object()
			.ok_or_else(|| de::Error::custom("rules must be a JSON object"))?;
		if obj.len() != 1 {
			return Err(de::Error::custom(
				"rules must be a single-key {\"if\": …} object",
			));
		}
		let (key, args) = obj.iter().next().expect("len == 1");
		if key != "if" {
			return Err(de::Error::custom(format!(
				"rules op must be 'if' (got '{key}')"
			)));
		}
		let args = args
			.as_array()
			.ok_or_else(|| de::Error::custom("'if' args must be an array"))?;
		if args.is_empty() {
			return Err(de::Error::custom(
				"'if' args must be non-empty; clear `rules` to remove the ladder instead",
			));
		}
		if args.len() % 2 != 0 {
			return Err(de::Error::custom(
				"'if' args must have even length (alternating condition, severity); \
				 a trailing else is not allowed",
			));
		}
		let mut branches = Vec::with_capacity(args.len() / 2);
		for chunk in args.chunks_exact(2) {
			let cond: Condition =
				serde_json::from_value(chunk[0].clone()).map_err(de::Error::custom)?;
			let sev_str = chunk[1].as_str().ok_or_else(|| {
				de::Error::custom("each odd-indexed 'if' arg must be a severity string")
			})?;
			let sev: Severity = sev_str
				.parse()
				.map_err(|_| de::Error::custom(format!("invalid severity '{sev_str}'")))?;
			branches.push((cond, sev));
		}
		Ok(IfLadder { branches })
	}
}
