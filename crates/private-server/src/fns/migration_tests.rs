use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use database::migration_tests::GroupVerdict;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new().routes(routes!(for_group))
}

/// Request body for reading a group's migration-test verdicts.
#[derive(Deserialize, ToSchema)]
pub struct ForGroupArgs {
	/// The group to report on.
	pub group_id: Uuid,
}

/// Where each of a group's servers stands against the version it would take
/// next.
///
/// One entry per server that has a candidate version. A server already on the
/// newest published version, running another product, or yet to report a
/// version has nothing to be tested against and is absent.
// spec: RST#verdicts
#[utoipa::path(
	post,
	path = "/for_group",
	operation_id = "migration_tests_for_group",
	tag = "migration_tests",
	security(("tailscale-admin" = [])),
	request_body = ForGroupArgs,
	responses(
		(status = 200, description = "Verdicts, one per server with a candidate.", body = Vec<GroupVerdict>),
		(status = 401, body = ProblemDetailsSchema),
		(status = 403, body = ProblemDetailsSchema),
	),
)]
pub async fn for_group(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<ForGroupArgs>,
) -> Result<Json<Vec<GroupVerdict>>> {
	let mut conn = state.db.get().await?;
	let verdicts = database::migration_tests::verdicts_for_group(&mut conn, args.group_id).await?;
	Ok(Json(verdicts))
}
