//! `list_upgrade_plans` / `get_upgrade_plan_history` tools.

use std::collections::HashMap;

use commons_types::{Uuid, version::VersionStr};
use database::{
	server_groups::ServerGroup,
	upgrade_plans::{PlanOutcome, UpgradePlan},
	versions::Version,
};
use jiff::{Timestamp, Zoned, civil::Date, civil::Time};
use rmcp::{
	handler::server::wrapper::Parameters,
	model::{CallToolResult, ErrorData as McpError},
	tool, tool_router,
};
use serde::Serialize;

use crate::{
	CanopyMcp,
	groups::GroupIdArgs,
	util::{EmptyArgs, mcp_err, not_found, ok_json, parse_uuid},
};

#[derive(Serialize)]
struct PlanList {
	plans: Vec<OpenPlan>,
	groups_without_a_plan: Vec<GroupRef>,
}

#[derive(Serialize)]
struct GroupRef {
	group_id: Uuid,
	group_name: String,
	current_version: Option<VersionStr>,
}

#[derive(Serialize)]
struct OpenPlan {
	group_id: Uuid,
	group_name: String,
	current_version: Option<VersionStr>,
	target_version: String,
	planned_for: Option<Date>,
	/// The hour it starts on the planned day, as a wall clock in
	/// `planned_zone`.
	planned_time: Option<Time>,
	/// The hour the window closes. Earlier than the start means the next
	/// morning.
	planned_end_time: Option<Time>,
	planned_zone: Option<String>,
	/// The planned day has passed and the deployment has not moved.
	late: bool,
	note: Option<String>,
	recorded_by: Option<String>,
	recorded_at: Timestamp,
}

#[derive(Serialize)]
struct PlanHistory {
	group_id: Uuid,
	group_name: String,
	current_version: Option<VersionStr>,
	plans: Vec<HistoricPlan>,
}

#[derive(Serialize)]
struct HistoricPlan {
	target_version: String,
	outcome: PlanOutcome,
	planned_for: Option<Date>,
	/// The hour it started on the planned day, as a wall clock in
	/// `planned_zone`.
	planned_time: Option<Time>,
	/// The hour the window closed. Earlier than the start means the next
	/// morning.
	planned_end_time: Option<Time>,
	planned_zone: Option<String>,
	note: Option<String>,
	recorded_by: Option<String>,
	recorded_at: Timestamp,
	amended_by: Option<String>,
	amended_at: Option<Timestamp>,
	withdrawn_by: Option<String>,
	/// When the plan closed, absent while it is open.
	ended_at: Option<Timestamp>,
}

#[tool_router(router = upgrade_plans_router, vis = "pub(crate)")]
impl CanopyMcp {
	#[tool(
		description = "Where every deployment is going: each group's open upgrade plan with the \
		               version it runs now, the version it plans to move to, the planned date, and \
		               whether that date has passed unmet. Groups with nothing recorded are \
		               returned separately."
	)]
	async fn list_upgrade_plans(
		&self,
		Parameters(_): Parameters<EmptyArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let today = Zoned::now().date();
		let versions = version_names(&mut conn).await?;

		let mut plans = Vec::new();
		let mut unplanned = Vec::new();
		for group in ServerGroup::list_all(&mut conn).await.map_err(mcp_err)? {
			let plan = UpgradePlan::open_for_group(&mut conn, group.id)
				.await
				.map_err(mcp_err)?;
			match plan {
				Some(plan) => plans.push(OpenPlan {
					group_id: group.id,
					group_name: group.name,
					current_version: group.effective_version,
					target_version: version_name(&versions, plan.target_version_id),
					late: database::upgrade_plans::is_late(&plan, today),
					planned_for: plan.planned_for,
					planned_time: plan.planned_time,
					planned_end_time: plan.planned_end_time,
					planned_zone: plan.planned_zone,
					note: plan.note,
					recorded_by: plan.created_by,
					recorded_at: plan.created_at,
				}),
				None => unplanned.push(GroupRef {
					group_id: group.id,
					group_name: group.name,
					current_version: group.effective_version,
				}),
			}
		}

		ok_json(&PlanList {
			plans,
			groups_without_a_plan: unplanned,
		})
	}

	#[tool(
		description = "Every upgrade plan one group has had, newest first, with how each stands: \
		               open, met (the group reached the target), replaced by a later plan, or \
		               withdrawn (an operator said the deployment is no longer going there)."
	)]
	async fn get_upgrade_plan_history(
		&self,
		Parameters(args): Parameters<GroupIdArgs>,
	) -> Result<CallToolResult, McpError> {
		let mut conn = self.conn().await?;
		let id = parse_uuid(&args.group_id, "group_id")?;
		let Ok(group) = ServerGroup::get_by_id(&mut conn, id).await else {
			return Ok(not_found(format!("no group with id {id}")));
		};

		let versions = version_names(&mut conn).await?;
		let plans = UpgradePlan::history_for_group(&mut conn, id)
			.await
			.map_err(mcp_err)?
			.into_iter()
			.map(|plan| HistoricPlan {
				target_version: version_name(&versions, plan.target_version_id),
				outcome: database::upgrade_plans::outcome(&plan),
				ended_at: database::upgrade_plans::ended_at(&plan),
				planned_for: plan.planned_for,
				planned_time: plan.planned_time,
				planned_end_time: plan.planned_end_time,
				planned_zone: plan.planned_zone,
				note: plan.note,
				recorded_by: plan.created_by,
				recorded_at: plan.created_at,
				amended_by: plan.amended_by,
				amended_at: plan.amended_at,
				withdrawn_by: plan.withdrawn_by,
			})
			.collect();

		ok_json(&PlanHistory {
			group_id: group.id,
			group_name: group.name,
			current_version: group.effective_version,
			plans,
		})
	}
}

/// Drafts included: a target yanked since the plan was recorded is still where
/// the deployment was going.
async fn version_names(
	conn: &mut database::diesel_async::AsyncPgConnection,
) -> Result<HashMap<Uuid, String>, McpError> {
	Ok(Version::get_all_including_drafts(conn)
		.await
		.map_err(mcp_err)?
		.into_iter()
		.map(|version| (version.id, version.as_semver().to_string()))
		.collect())
}

fn version_name(versions: &HashMap<Uuid, String>, id: Uuid) -> String {
	versions
		.get(&id)
		.cloned()
		.unwrap_or_else(|| "unknown".into())
}
