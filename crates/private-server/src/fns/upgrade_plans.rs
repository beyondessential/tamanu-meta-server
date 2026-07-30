use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use database::upgrade_plans::UpgradePlan;
use jiff::{Zoned, civil::Date};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(fleet))
		.routes(routes!(for_group))
		.routes(routes!(record))
		.routes(routes!(withdraw))
}

/// One row of the planned-upgrades view.
#[derive(Serialize, ToSchema)]
pub struct PlannedUpgrade {
	/// The group this concerns.
	pub group_id: Uuid,
	/// Its name, so the view reads without a second lookup.
	pub group_name: String,
	/// The version the group runs now, where it has reported one.
	pub current_version: Option<String>,
	/// The plan, absent for a group with none.
	pub plan: Option<UpgradePlan>,
	/// The plan's target as semver.
	pub target_version: Option<String>,
	/// Whether the planned date has passed without the upgrade happening.
	/// Presentational: a slipping upgrade is normal operational reality.
	pub late: bool,
}

/// Planned upgrades across the fleet.
///
/// Every live group, whether or not it has a plan. A group several minors
/// behind with no plan is the thing this view exists to surface, so it is listed
/// rather than omitted.
// spec: UPG#the-dashboard
#[utoipa::path(
	post,
	path = "/fleet",
	operation_id = "upgrade_plans_fleet",
	tag = "upgrade_plans",
	security(("tailscale-admin" = [])),
	responses(
		(status = 200, description = "One row per live group.", body = Vec<PlannedUpgrade>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn fleet(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
) -> Result<Json<Vec<PlannedUpgrade>>> {
	let mut conn = state.db.get().await?;
	// Late is judged against the server's own day; nothing depends on it beyond
	// presentation.
	let today = Zoned::now().date();

	let mut out = Vec::new();
	for group in database::server_groups::ServerGroup::list_all(&mut conn).await? {
		let plan = UpgradePlan::open_for_group(&mut conn, group.id).await?;
		let target = match &plan {
			Some(plan) => Some(
				database::upgrade_plans::target_version_str(&mut conn, plan)
					.await?
					.to_string(),
			),
			None => None,
		};
		let late = plan
			.as_ref()
			.is_some_and(|plan| database::upgrade_plans::is_late(plan, today));

		out.push(PlannedUpgrade {
			group_id: group.id,
			group_name: group.name,
			current_version: group.effective_version.map(|v| v.to_string()),
			plan,
			target_version: target,
			late,
		});
	}

	Ok(Json(out))
}

/// Request body for reading one group's plans. Named apart from the
/// migration-test one because utoipa keys component schemas by short name.
#[derive(Deserialize, ToSchema)]
pub struct PlansForGroupArgs {
	/// The group to read.
	pub group_id: Uuid,
}

/// A group's plan and the plans it has had before.
///
/// The history is the record of what a deployment planned, when for, and when
/// it landed.
// spec: UPG#when-a-plan-is-met
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "upgrade_plans_for_group",
	tag = "upgrade_plans",
	security(("tailscale-admin" = [])),
	request_body = PlansForGroupArgs,
	responses(
		(status = 200, description = "Every plan the group has had, newest first.", body = Vec<UpgradePlan>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn for_group(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<PlansForGroupArgs>,
) -> Result<Json<Vec<UpgradePlan>>> {
	let mut conn = state.db.get().await?;
	Ok(Json(
		UpgradePlan::history_for_group(&mut conn, args.group_id).await?,
	))
}

/// Request body for recording where a group is going.
#[derive(Deserialize, ToSchema)]
pub struct RecordArgs {
	/// The group that intends to move.
	pub group_id: Uuid,
	/// The published version it intends to move to.
	pub target_version_id: Uuid,
	/// The day it is expected to happen, as `YYYY-MM-DD`. Optional.
	#[schema(value_type = Option<String>)]
	pub planned_for: Option<Date>,
	/// Anything the next reader needs to know. Optional.
	pub note: Option<String>,
}

/// Record where a group is going, retiring any plan it already had.
///
/// A group goes one place next, so this replaces rather than queues. The target
/// must be published and ahead of what the group runs.
// spec: UPG#a-plan
#[utoipa::path(
	post,
	path = "/record",
	operation_id = "upgrade_plans_record",
	tag = "upgrade_plans",
	security(("tailscale-admin" = [])),
	request_body = RecordArgs,
	responses(
		(status = 200, description = "The recorded plan.", body = UpgradePlan),
		(status = 400, description = "The target is unpublished, or not ahead of the group.", body = ProblemDetailsSchema),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn record(
	State(state): State<AppState>,
	admin: TailscaleAdmin,
	Json(args): Json<RecordArgs>,
) -> Result<Json<UpgradePlan>> {
	let mut conn = state.db.get().await?;
	let note = args
		.note
		.as_deref()
		.map(str::trim)
		.filter(|n| !n.is_empty());
	let plan = UpgradePlan::record(
		&mut conn,
		args.group_id,
		args.target_version_id,
		args.planned_for,
		note,
		&admin.0.login,
	)
	.await?;
	Ok(Json(plan))
}

/// Request body for withdrawing a plan.
#[derive(Deserialize, ToSchema)]
pub struct WithdrawArgs {
	/// The plan to withdraw.
	pub id: Uuid,
}

/// Withdraw a plan: the deployment is no longer going there.
///
/// This does not say the upgrade happened. Canopy closes a met plan on its own
/// once the group reports the target.
// spec: UPG#a-plan
#[utoipa::path(
	post,
	path = "/withdraw",
	operation_id = "upgrade_plans_withdraw",
	tag = "upgrade_plans",
	security(("tailscale-admin" = [])),
	request_body = WithdrawArgs,
	responses(
		(status = 200, description = "Withdrawn (idempotent)."),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn withdraw(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<WithdrawArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	UpgradePlan::delete(&mut conn, args.id).await?;
	Ok(Json(()))
}
