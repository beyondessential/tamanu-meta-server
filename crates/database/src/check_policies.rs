//! Operator-owned catalog of check policies: per (source, check), how an
//! observed result transforms into the effective result Canopy acts on.
//!
//! An entry carries a **ceiling** (the maximum effective result on the
//! urgency ordering — `failed` changes nothing, `warning` grades failures
//! as warnings, `passed` means recorded but never alerting, `skipped`
//! additionally tells the source not to bother running the check),
//! optional conditional **rules** (transforms in any direction, evaluated
//! against the check's detail, the report's server-wide detail, and the
//! server's tags), and an **escalates** flag (an effective failure of
//! this check notifies immediately, bypassing incident grace).
//!
//! Ingestion (in the public-server status handler) calls
//! [`CheckPolicy::upsert_default`] for every check seen on a push, then
//! [`CheckPolicy::apply`] to grade each observed result. Operators read
//! and edit the catalog via the private-server `/api/healthchecks`
//! endpoints.

use crate::issues::Scope;
use commons_errors::{AppError, Result};
use commons_types::status::CheckResult;
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::Timestamp;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// A catalogued check unreported anywhere in the fleet for this long is
/// surfaced to operators as a candidate for decommissioning.
pub const GONE_QUIET_HOURS: i64 = 24 * 7;
/// A catalogued check unreported anywhere in the fleet for this long
/// raises a canopy-wide warning.
pub const STALE_ALERT_HOURS: i64 = 24 * 30;

/// The policy for one (source, check). An entry is created automatically
/// the first time a source reports a check with this name, at the default
/// ceiling; operators then review and adjust how that check's results are
/// graded going forward.
#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::check_policies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct CheckPolicy {
	/// The source that reports this check. Together with `check_name`,
	/// uniquely identifies this policy.
	pub source: String,
	/// The check's name, as reported in status pushes.
	pub check_name: String,
	/// The maximum effective result for this check when no conditional
	/// rule (see `rules`) overrides it: an observed result more urgent
	/// than the ceiling grades down to it.
	#[diesel(deserialize_as = String, serialize_as = String)]
	#[schema(value_type = String)]
	pub ceiling: CheckResult,
	/// Whether an effective failure of this check notifies immediately,
	/// bypassing the incident grace period.
	pub escalates: bool,
	/// Optional conditional rules that can grade a result differently —
	/// in any direction — depending on the details of the check, the
	/// surrounding status report, or the server's tags. `None` means no
	/// conditional rules are configured and the ceiling always applies.
	/// When present, the rules are evaluated in order and the first
	/// matching one wins; if none match, the ceiling applies.
	#[schema(value_type = Option<serde_json::Value>)]
	pub rules: Option<JsonValue>,
	/// Free-form operator notes about this check.
	pub notes: Option<String>,
	/// When this check was first observed and this policy entry was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub first_seen: Timestamp,
	/// When an operator last reviewed or edited this policy. `None` if it
	/// has never been reviewed.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub reviewed_at: Option<Timestamp>,
	/// The operator who last reviewed this policy. `None` if it has never
	/// been reviewed.
	pub reviewed_by: Option<String>,
	/// When this policy was last modified.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// Operator-authored documentation for this check: a single markdown
	/// document, presented wherever the check's state is presented and
	/// over MCP. By convention it covers what the check observes, what
	/// each result means, and hints for solving a failure; canopy
	/// attaches no meaning to its structure.
	pub documentation: Option<String>,
	/// When this check was most recently reported on any server across the
	/// fleet. Reconciled periodically from check state, not stamped on
	/// ingestion. `None` until the first reconcile after the check appears.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub last_seen: Option<Timestamp>,
	/// When an operator decommissioned this check. A decommissioned check
	/// contributes to nothing — health, incidents, or source staleness —
	/// until it is reported again. `None` while live.
	#[diesel(deserialize_as = jiff_diesel::NullableTimestamp, serialize_as = jiff_diesel::NullableTimestamp)]
	pub decommissioned_at: Option<Timestamp>,
	/// The operator who decommissioned this check. `None` while live.
	pub decommissioned_by: Option<String>,
}

/// Escalation only makes sense at a `failed` ceiling: it bypasses
/// incident grace on an effective *failure*, and only a `failed` ceiling
/// lets an effective result reach failed in the first place. At any lower
/// ceiling the flag can never fire, so it is dropped rather than stored as
/// dead configuration. The `check_policies` schema enforces the same
/// invariant with a check constraint.
fn escalates_normalised(ceiling: CheckResult, escalates: bool) -> bool {
	escalates && ceiling == CheckResult::Failed
}

/// The outcome of applying a check's policy to an observed result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GradedResult {
	/// The effective result after the policy transform.
	pub effective: CheckResult,
	/// Whether this check's effective failures escalate (notify
	/// immediately, bypassing incident grace).
	pub escalates: bool,
}

impl CheckPolicy {
	/// Reconcile fleet-wide check liveness. Refreshes every catalogued
	/// `(source, check)`'s `last_seen` to the most recent report of that
	/// check on any server (synthetic `canopy`/`manual` sources excluded),
	/// and re-animates any decommissioned check that has been reported
	/// since it was retired: cleared back to the newly-registered state
	/// (warning ceiling, pending review) so a resurrected check never
	/// silently resumes a retired policy. Runs periodically off the hot
	/// ingestion path — a few minutes of lag is fine. Returns the number of
	/// checks re-animated.
	pub async fn reconcile_liveness(db: &mut AsyncPgConnection) -> Result<usize> {
		use diesel::sql_query;

		// Refresh last_seen from current check state: one row per
		// (server, source, check) carries that check's most recent report
		// time, so the fleet-wide max per (source, check) is its liveness.
		let refresh = format!(
			"UPDATE check_policies cp SET last_seen = f.max_seen FROM (\
			 SELECT source, check_name, max(last_seen) AS max_seen FROM issues \
			 WHERE server_id IS NOT NULL AND check_name IS NOT NULL \
			 AND source NOT IN ('{canopy}', '{manual}') \
			 GROUP BY source, check_name) f \
			 WHERE cp.source = f.source AND cp.check_name = f.check_name \
			 AND (cp.last_seen IS NULL OR cp.last_seen < f.max_seen)",
			canopy = crate::statuses::CANOPY_SOURCE,
			manual = crate::issues::MANUAL_SOURCE,
		);
		sql_query(refresh).execute(db).await?;

		// Re-animate any decommissioned check that has reported since it was
		// retired: reset to the newly-registered state (warning ceiling, no
		// escalation, pending review, no rules) so its policy is re-vetted.
		// Escalation must clear alongside the drop to the warning ceiling —
		// it is only valid at a failed ceiling (enforced by a check
		// constraint).
		let reanimated = sql_query(
			"UPDATE check_policies SET decommissioned_at = NULL, decommissioned_by = NULL, \
			 ceiling = 'warning', escalates = FALSE, rules = NULL, \
			 reviewed_at = NULL, reviewed_by = NULL \
			 WHERE decommissioned_at IS NOT NULL AND last_seen IS NOT NULL \
			 AND last_seen > decommissioned_at",
		)
		.execute(db)
		.await?;

		Ok(reanimated)
	}

	/// The set of `(source, check)` pairs backed by a **live** catalog row
	/// (present and not decommissioned). Callers require membership before
	/// treating a check-state as a real check — anything user-facing
	/// (health, presentation) ignores states with no live catalog row: both
	/// decommissioned checks and orphaned check-states (an `issues` row whose
	/// (source, check) has no catalog row at all, e.g. a superseded source
	/// whose catalog rows were removed while its states lingered).
	pub async fn live_cataloged_pairs(
		db: &mut AsyncPgConnection,
	) -> Result<std::collections::HashSet<(String, String)>> {
		use crate::schema::check_policies::dsl;
		Ok(dsl::check_policies
			.select((dsl::source, dsl::check_name))
			.filter(dsl::decommissioned_at.is_null())
			.load::<(String, String)>(db)
			.await?
			.into_iter()
			.collect())
	}

	/// Live (not decommissioned) catalogued checks whose most recent
	/// fleet-wide report is older than `cutoff`. Ordered by source then
	/// name. Drives the operator "gone quiet" list and the stale-check
	/// self-alert.
	pub async fn gone_quiet(db: &mut AsyncPgConnection, cutoff: Timestamp) -> Result<Vec<Self>> {
		use crate::schema::check_policies::dsl;
		dsl::check_policies
			.select(Self::as_select())
			.filter(dsl::decommissioned_at.is_null())
			.filter(dsl::last_seen.is_not_null())
			.filter(dsl::last_seen.lt(jiff_diesel::Timestamp::from(cutoff)))
			.order((dsl::source, dsl::check_name))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Decommission a `(source, check)` fleet-wide: mark the catalog row,
	/// resolve the check's outstanding states on every server (recording
	/// decommissioning as the reason, re-evaluating incident membership),
	/// and — when the source has no live checks left — clear its per-server
	/// staleness so a retired reporter stops being flagged. Operator action.
	pub async fn decommission(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		by: &str,
	) -> Result<()> {
		use crate::schema::check_policies::dsl as cp;
		use crate::schema::issues::dsl as iss;
		use commons_types::issue::ResolvedReason;

		let now = jiff_diesel::Timestamp::from(Timestamp::now());
		diesel::update(
			cp::check_policies.filter(cp::source.eq(source).and(cp::check_name.eq(check_name))),
		)
		.set((
			cp::decommissioned_at.eq(Some(now)),
			cp::decommissioned_by.eq(Some(by)),
		))
		.execute(db)
		.await?;

		// Resolve the check's outstanding states on every server, so they
		// stop counting toward health and incidents.
		let state_ids: Vec<Uuid> = iss::issues
			.select(iss::id)
			.filter(iss::source.eq(source))
			.filter(iss::check_name.eq(check_name))
			.filter(iss::server_id.is_not_null())
			.filter(iss::resolved_at.is_null())
			.load(db)
			.await?;
		for id in state_ids {
			crate::issues::Issue::resolve(db, id, by, ResolvedReason::Decommissioned).await?;
		}

		// Clear any scoped silences for the now-dead check: a silence on a
		// check that contributes to nothing is dead configuration, and it
		// would otherwise linger in the operator's silence list forever.
		use crate::schema::scoped_check_policies::dsl as scp;
		diesel::delete(
			scp::scoped_check_policies
				.filter(scp::source.eq(source).and(scp::check_name.eq(check_name))),
		)
		.execute(db)
		.await?;

		// A source whose checks are all decommissioned drops out of
		// source_freshness, so it stops counting toward reachability with no
		// further action here.
		Ok(())
	}

	/// Insert a row for `(source, check_name)` with default values
	/// (ceiling = warning) if and only if no row exists yet. Idempotent:
	/// safe to call on every status push for every check seen, including
	/// healthy ones. Concurrent pushes are serialised by Postgres via
	/// `ON CONFLICT DO NOTHING`.
	pub async fn upsert_default(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
	) -> Result<()> {
		use crate::schema::check_policies::dsl;
		diesel::insert_into(dsl::check_policies)
			.values((dsl::source.eq(source), dsl::check_name.eq(check_name)))
			.on_conflict((dsl::source, dsl::check_name))
			.do_nothing()
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Insert a row for `(source, check_name)` with the given policy —
	/// and, for canopy's own checks, shipped documentation — if and only
	/// if no row exists yet. Canopy's own checks register with the policy
	/// their condition warrants instead of the default warning ceiling;
	/// operator edits stick (this never overwrites).
	///
	/// Registered checks are canopy's own or operator-raised (never
	/// un-vetted device pushes, which go through [`Self::upsert_default`]),
	/// so they register **already reviewed** (`reviewed_at` stamped) — they
	/// alert at their real ceiling rather than being capped at warning like
	/// a pending device check (see [`Self::apply`] and the CHK spec).
	pub async fn register(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		ceiling: CheckResult,
		escalates: bool,
		documentation: Option<&str>,
	) -> Result<()> {
		use crate::schema::check_policies::dsl;
		let now = jiff_diesel::Timestamp::from(Timestamp::now());
		diesel::insert_into(dsl::check_policies)
			.values((
				dsl::source.eq(source),
				dsl::check_name.eq(check_name),
				dsl::ceiling.eq(ceiling.to_string()),
				dsl::escalates.eq(escalates_normalised(ceiling, escalates)),
				dsl::documentation.eq(documentation),
				dsl::reviewed_at.eq(Some(now)),
				dsl::reviewed_by.eq(Some(source)),
			))
			.on_conflict((dsl::source, dsl::check_name))
			.do_nothing()
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Apply the `(source, check_name)` policy to an `observed` result:
	/// if the entry has a `rules` ladder and a branch matches the
	/// supplied evaluation context, that branch's result wins (any
	/// direction — rules can upgrade as well as downgrade); otherwise the
	/// observed result is capped at the entry's ceiling.
	///
	/// A check still **pending operator review** (`reviewed_at IS NULL`)
	/// has never been vetted, so it must not alert: its effective result is
	/// hard-capped at warning (and warnings never open incidents),
	/// whatever its ceiling or rules say. Reviewing the policy — even a
	/// no-op save — lifts the cap.
	///
	/// Falls back to the default policy (ceiling = warning, no
	/// escalation) if no row exists yet — in practice the status handler
	/// upserts before reading, so this branch only covers the genuine
	/// race / programmer-error case.
	pub async fn apply(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		observed: CheckResult,
		ctx: &EvaluationContext<'_>,
	) -> Result<GradedResult> {
		use crate::schema::check_policies::dsl;
		let row: Option<(String, bool, Option<JsonValue>, bool)> = dsl::check_policies
			.select((
				dsl::ceiling,
				dsl::escalates,
				dsl::rules,
				dsl::reviewed_at.is_not_null(),
			))
			.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name)))
			.first(db)
			.await
			.optional()?;
		let Some((ceiling_str, escalates, rules_json, reviewed)) = row else {
			return Ok(GradedResult {
				effective: observed.capped_at(CheckResult::Warning),
				escalates: false,
			});
		};
		let ceiling = ceiling_str.parse().unwrap_or(CheckResult::Warning);
		let mut effective = observed.capped_at(ceiling);
		if let Some(rules) = rules_json {
			match serde_json::from_value::<IfLadder>(rules) {
				Ok(ladder) => {
					if let Some(result) = ladder.evaluate(ctx) {
						effective = result;
					}
				}
				Err(err) => {
					tracing::warn!(
						source,
						check_name,
						?err,
						"failed to parse check policy rules; falling back to ceiling"
					);
				}
			}
		}
		// A never-reviewed check is inert until an operator vets it.
		if !reviewed {
			effective = effective.capped_at(CheckResult::Warning);
		}
		Ok(GradedResult {
			effective,
			escalates,
		})
	}

	/// [`Self::apply`], then the scoped transforms that cover the filing's
	/// target: fleet catalog, then group, then server — each acting on
	/// the previous effective result, so the most specific scope has the
	/// last word. A canopy-wide filing (no server, no group) chains the
	/// canopy-wide scoped transform instead.
	///
	/// A scoped silence is a skipped ceiling in this chain: whatever the
	/// fleet grading said, the effective result lands at skipped.
	///
	/// The pending-review warning cap (see [`Self::apply`]) is applied to
	/// the fleet grade. Scoped transforms could in principle grade back up,
	/// but the only surface that creates them is the silence (a skipped
	/// ceiling, which only narrows), so a pending check stays capped in
	/// every reachable state.
	pub async fn apply_scoped(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		observed: CheckResult,
		ctx: &EvaluationContext<'_>,
		server_id: Option<Uuid>,
		group_id: Option<Uuid>,
	) -> Result<GradedResult> {
		let fleet = Self::apply(db, source, check_name, observed, ctx).await?;
		let scoped =
			ScopedCheckPolicy::chain_for(db, source, check_name, server_id, group_id).await?;
		let mut effective = fleet.effective;
		for transform in &scoped {
			effective = transform.transform(effective, ctx);
		}
		Ok(GradedResult {
			effective,
			escalates: fleet.escalates,
		})
	}

	/// Replace the documentation for a check (or clear it with `None`).
	/// Doesn't stamp the review columns — documenting a check is not the
	/// same as reviewing its policy.
	pub async fn update_documentation(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		documentation: Option<&str>,
	) -> Result<Self> {
		use crate::schema::check_policies::dsl;
		diesel::update(
			dsl::check_policies.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name))),
		)
		.set(dsl::documentation.eq(documentation))
		.returning(Self::as_select())
		.get_result(db)
		.await
		.map_err(AppError::from)
	}

	/// Replace the conditional-rules ladder for a check (or clear it
	/// with `None`). Stamps `reviewed_at` / `reviewed_by`, so editing
	/// rules also counts as a review for the catalog row.
	pub async fn update_rules(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		rules: Option<&IfLadder>,
		by: &str,
	) -> Result<Self> {
		use crate::schema::check_policies::dsl;
		let now = Timestamp::now();
		let rules_json: Option<JsonValue> =
			rules.map(|l| serde_json::to_value(l).expect("IfLadder always serialises"));
		diesel::update(
			dsl::check_policies.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name))),
		)
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

	/// One source's catalog as `check_name → ceiling`, for building the
	/// device-facing effective check map. Deliberately reads only the
	/// static `ceiling` column: conditional `rules` ladders are
	/// expressions evaluated per push against the report's contents, so
	/// they can't be resolved ahead of time and are ignored here. An
	/// unparseable ceiling falls back to warning, same as [`Self::apply`].
	pub async fn ceiling_map_for_source(
		db: &mut AsyncPgConnection,
		source: &str,
	) -> Result<BTreeMap<String, CheckResult>> {
		use crate::schema::check_policies::dsl;
		let rows: Vec<(String, String)> = dsl::check_policies
			.select((dsl::check_name, dsl::ceiling))
			.filter(dsl::source.eq(source))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows
			.into_iter()
			.map(|(name, ceiling)| (name, ceiling.parse().unwrap_or(CheckResult::Warning)))
			.collect())
	}

	pub async fn list(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::check_policies::dsl;
		dsl::check_policies
			.select(Self::as_select())
			.order((dsl::source.asc(), dsl::check_name.asc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// The catalog row for a single (source, check), or `None` if that
	/// source has never reported it (so ingestion has never upserted a
	/// row).
	pub async fn get(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
	) -> Result<Option<Self>> {
		use crate::schema::check_policies::dsl;
		dsl::check_policies
			.select(Self::as_select())
			.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name)))
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Update the ceiling, escalation flag, and optionally notes for a
	/// check, stamping `reviewed_at = NOW()` and `reviewed_by = by`. Even
	/// a no-op save marks the row reviewed — operators can ack a check
	/// without changing it.
	///
	/// Escalation is only meaningful at a `failed` ceiling — it bypasses
	/// incident grace on an effective failure, and only a `failed` ceiling
	/// admits a failed effective result — so it is dropped at any lower
	/// ceiling (see [`escalates_normalised`]).
	pub async fn update(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		ceiling: CheckResult,
		escalates: bool,
		notes: Option<&str>,
		by: &str,
	) -> Result<Self> {
		use crate::schema::check_policies::dsl;
		let now = Timestamp::now();
		diesel::update(
			dsl::check_policies.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name))),
		)
		.set((
			dsl::ceiling.eq(ceiling.to_string()),
			dsl::escalates.eq(escalates_normalised(ceiling, escalates)),
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

/// A result transform scoped to one target, applied after the fleet
/// catalog: fleet, then group, then server, each acting on the previous
/// effective result. Either side may be present — a ceiling, a rules
/// ladder, or both (rules first, then the ceiling caps their outcome).
///
/// The operator-facing **silence** is a scoped ceiling of `skipped`:
/// the check keeps recording observed results but its effective result
/// is skipped, so it raises nothing and counts nowhere. Arbitrary
/// scoped transforms are admitted here; the UI only offers silences.
#[derive(Clone, Debug, Serialize, Deserialize, Queryable, Selectable, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::scoped_check_policies)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ScopedCheckPolicy {
	/// Unique identifier of this scoped transform.
	pub id: Uuid,
	/// When this transform was created.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub created_at: Timestamp,
	/// When this transform was last modified.
	#[diesel(deserialize_as = jiff_diesel::Timestamp, serialize_as = jiff_diesel::Timestamp)]
	pub updated_at: Timestamp,
	/// The source whose check this transform applies to.
	pub source: String,
	/// The check this transform applies to.
	pub check_name: String,
	/// Set for a server-scoped transform.
	pub server_id: Option<Uuid>,
	/// Set for a group-scoped transform. Both `server_id` and
	/// `server_group_id` unset means canopy-wide scope.
	pub server_group_id: Option<Uuid>,
	/// Scoped ceiling: caps the effective result arriving from the
	/// previous transform in the chain. `skipped` is the silence.
	pub ceiling: Option<String>,
	/// Scoped conditional rules, same shape as the fleet catalog's.
	#[schema(value_type = Option<serde_json::Value>)]
	pub rules: Option<JsonValue>,
	/// The operator who created this transform. `None` if not recorded.
	pub created_by: Option<String>,
}

impl ScopedCheckPolicy {
	/// The transform at exactly this (scope, source, check), if any.
	pub async fn get(
		db: &mut AsyncPgConnection,
		scope: Scope,
		source: &str,
		check_name: &str,
	) -> Result<Option<Self>> {
		use crate::schema::scoped_check_policies::dsl;
		let (server, group) = scope.to_columns();
		dsl::scoped_check_policies
			.select(Self::as_select())
			.filter(
				dsl::source
					.eq(source)
					.and(dsl::check_name.eq(check_name))
					.and(dsl::server_id.is_not_distinct_from(server))
					.and(dsl::server_group_id.is_not_distinct_from(group)),
			)
			.first(db)
			.await
			.optional()
			.map_err(AppError::from)
	}

	/// Upsert a silence: a skipped ceiling at this scope. An existing
	/// transform at the same (scope, source, check) keeps its rules; its
	/// ceiling becomes skipped. Idempotent.
	pub async fn silence(
		db: &mut AsyncPgConnection,
		scope: Scope,
		source: &str,
		check_name: &str,
		created_by: Option<&str>,
	) -> Result<Self> {
		use crate::schema::scoped_check_policies::dsl;
		let (server, group) = scope.to_columns();
		if let Some(existing) = Self::get(db, scope, source, check_name).await? {
			return diesel::update(dsl::scoped_check_policies.filter(dsl::id.eq(existing.id)))
				.set((
					dsl::ceiling.eq(CheckResult::Skipped.to_string()),
					dsl::updated_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				))
				.returning(Self::as_select())
				.get_result(db)
				.await
				.map_err(AppError::from);
		}
		diesel::insert_into(dsl::scoped_check_policies)
			.values((
				dsl::source.eq(source),
				dsl::check_name.eq(check_name),
				dsl::server_id.eq(server),
				dsl::server_group_id.eq(group),
				dsl::ceiling.eq(CheckResult::Skipped.to_string()),
				dsl::created_by.eq(created_by),
			))
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)
	}

	/// Remove a silence at this scope: the row is deleted when the
	/// silence was all it carried, or just the skipped ceiling is lifted
	/// when scoped rules remain. A no-op if nothing is silenced there.
	pub async fn unsilence(
		db: &mut AsyncPgConnection,
		scope: Scope,
		source: &str,
		check_name: &str,
	) -> Result<()> {
		use crate::schema::scoped_check_policies::dsl;
		let Some(existing) = Self::get(db, scope, source, check_name).await? else {
			return Ok(());
		};
		if existing.ceiling.as_deref() != Some("skipped") {
			return Ok(());
		}
		if existing.rules.is_some() {
			diesel::update(dsl::scoped_check_policies.filter(dsl::id.eq(existing.id)))
				.set((
					dsl::ceiling.eq(None::<String>),
					dsl::updated_at.eq(jiff_diesel::Timestamp::from(Timestamp::now())),
				))
				.execute(db)
				.await
				.map_err(AppError::from)?;
		} else {
			diesel::delete(dsl::scoped_check_policies.filter(dsl::id.eq(existing.id)))
				.execute(db)
				.await
				.map_err(AppError::from)?;
		}
		Ok(())
	}

	/// All silences (skipped-ceiling transforms) at one scope, newest
	/// first. Silences for dead checks — a `(source, check)` with no live
	/// catalog row (decommissioned, or orphaned with no catalog row at all)
	/// — are excluded: the check contributes to nothing, so its silence is
	/// dead config that shouldn't clutter the operator's list.
	pub async fn list_silences(db: &mut AsyncPgConnection, scope: Scope) -> Result<Vec<Self>> {
		use crate::schema::scoped_check_policies::dsl;
		let (server, group) = scope.to_columns();
		let rows: Vec<Self> = dsl::scoped_check_policies
			.select(Self::as_select())
			.filter(
				dsl::server_id
					.is_not_distinct_from(server)
					.and(dsl::server_group_id.is_not_distinct_from(group))
					.and(dsl::ceiling.eq(CheckResult::Skipped.to_string())),
			)
			.order(dsl::created_at.desc())
			.load(db)
			.await
			.map_err(AppError::from)?;
		let cataloged = CheckPolicy::live_cataloged_pairs(db).await?;
		Ok(rows
			.into_iter()
			.filter(|r| cataloged.contains(&(r.source.clone(), r.check_name.clone())))
			.collect())
	}

	/// The scoped transforms that apply to a filing, in application
	/// order. A server filing chains group then server; a group filing
	/// its group row; a canopy-wide filing the global row.
	pub async fn chain_for(
		db: &mut AsyncPgConnection,
		source: &str,
		check_name: &str,
		server_id: Option<Uuid>,
		group_id: Option<Uuid>,
	) -> Result<Vec<Self>> {
		use crate::schema::scoped_check_policies::dsl;
		let mut query = dsl::scoped_check_policies
			.select(Self::as_select())
			.filter(dsl::source.eq(source).and(dsl::check_name.eq(check_name)))
			.into_boxed();
		query = match (server_id, group_id) {
			(None, None) => {
				query.filter(dsl::server_id.is_null().and(dsl::server_group_id.is_null()))
			}
			(server, group) => query.filter(
				dsl::server_id
					.is_not_distinct_from(server)
					.and(dsl::server_id.is_not_null())
					.or(dsl::server_group_id
						.is_not_distinct_from(group)
						.and(dsl::server_group_id.is_not_null())),
			),
		};
		let mut rows: Vec<Self> = query.load(db).await.map_err(AppError::from)?;
		// Group scope applies before server scope: the most specific
		// transform has the last word.
		rows.sort_by_key(|r| r.server_id.is_some());
		Ok(rows)
	}

	/// Apply this transform to the effective result arriving from the
	/// previous step in the chain: rules first (a matching branch's
	/// result replaces the input), then the ceiling caps the outcome.
	pub fn transform(&self, input: CheckResult, ctx: &EvaluationContext<'_>) -> CheckResult {
		let mut result = input;
		if let Some(rules) = &self.rules {
			match serde_json::from_value::<IfLadder>(rules.clone()) {
				Ok(ladder) => {
					if let Some(matched) = ladder.evaluate(ctx) {
						result = matched;
					}
				}
				Err(err) => {
					tracing::warn!(
						source = self.source,
						check_name = self.check_name,
						?err,
						"failed to parse scoped policy rules; ignoring them"
					);
				}
			}
		}
		if let Some(ceiling) = self.ceiling.as_deref().and_then(|c| c.parse().ok()) {
			result = result.capped_at(ceiling);
		}
		result
	}
}

// ── Rule model: JsonLogic-encoded if-ladder ────────────────────────────────

/// A JsonLogic `if`-ladder evaluating to a [`CheckResult`] string.
///
/// Wire shape: `{"if": [c1, r1, c2, r2, …, cN, rN]}` — even-length
/// argument list, every odd-index entry is a [`Condition`], every
/// even-index entry is a result literal. No trailing else: when no
/// branch matches, [`Self::evaluate`] returns `None` and the calling
/// [`CheckPolicy::apply`] falls through to the entry's ceiling.
///
/// Empty ladders are forbidden by the deserialiser; the API layer
/// normalises them to `None` (clearing the `rules` column) before
/// hitting the database.
#[derive(Clone, Debug, PartialEq)]
pub struct IfLadder {
	pub branches: Vec<(Condition, CheckResult)>,
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
	/// The check's own fields (the `health[i]` object minus `check` and
	/// `healthy`).
	pub check_extra: &'a serde_json::Map<String, JsonValue>,
	/// Server's resolved tag map (merged server + group). Each value is
	/// already wrapped as `JsonValue::String` for uniform comparison.
	pub tags: &'a HashMap<String, JsonValue>,
}

impl IfLadder {
	/// Returns the first matching branch's result, or `None` if no
	/// branch matches. The caller falls back to the entry's ceiling in
	/// that case.
	pub fn evaluate(&self, ctx: &EvaluationContext) -> Option<CheckResult> {
		self.branches
			.iter()
			.find_map(|(c, r)| c.matches(ctx).then_some(*r))
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
		for (c, r) in &self.branches {
			args.push(serde_json::to_value(c).map_err(serde::ser::Error::custom)?);
			args.push(JsonValue::String(r.to_string()));
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
				"'if' args must have even length (alternating condition, result); \
				 a trailing else is not allowed",
			));
		}
		let mut branches = Vec::with_capacity(args.len() / 2);
		for chunk in args.chunks_exact(2) {
			let cond: Condition =
				serde_json::from_value(chunk[0].clone()).map_err(de::Error::custom)?;
			let result_str = chunk[1].as_str().ok_or_else(|| {
				de::Error::custom("each odd-indexed 'if' arg must be a result string")
			})?;
			let result: CheckResult = result_str
				.parse()
				.map_err(|_| de::Error::custom(format!("invalid result '{result_str}'")))?;
			branches.push((cond, result));
		}
		Ok(IfLadder { branches })
	}
}
