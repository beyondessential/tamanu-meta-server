use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use database::migration_tests::GroupVerdict;
use serde::{Deserialize, Serialize};
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

/// Whether a restore attempt is under way for a group, for showing beside a
/// verdict.
///
/// Group-level, because credentials are issued per `(group, type)`: RST scopes
/// them that way since one repo holds all of a group's snapshots, so this is the
/// finest granularity the signal honestly has.
// spec: RST#verdicts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttemptState {
	/// Credentials were issued and have not expired with no report yet.
	InFlight,
	/// Credentials expired with no report: it ran and never said how it went.
	EndedWithoutReport,
}

/// The state of the newest unreported restore attempt for `group_id`, if any.
///
/// A restore takes hours, so without this a group mid-test reads as untested for
/// the whole window and a consumer that has stopped looks the same as one that
/// never started.
// spec: RST#verdicts
pub async fn attempt_state(
	conn: &mut database::diesel_async::AsyncPgConnection,
	group_id: Uuid,
	now: jiff::Timestamp,
) -> Result<Option<AttemptState>> {
	use database::backups::BackupCredentialIssuance;

	let reports =
		database::restore::BackupRestoreCheck::list_recent_for_group(conn, group_id, 50).await?;
	let since =
		crate::run_pairing::issuance_since(now, reports.iter().map(|c| c.reported_at).min());
	let issuances: Vec<_> =
		BackupCredentialIssuance::list_for_group_since(conn, group_id, since, 200)
			.await?
			.into_iter()
			.filter(|i| i.purpose == commons_types::backup::BackupPurpose::Restore)
			.collect();

	let report_refs: Vec<crate::run_pairing::ReportRef> = reports
		.iter()
		.map(|c| crate::run_pairing::ReportRef {
			run_id: c.run_id,
			key: crate::run_pairing::run_key(
				c.consumer_device_id,
				&c.r#type,
				commons_types::backup::BackupPurpose::Restore,
			),
			reported_at: c.reported_at,
		})
		.collect();

	let (_starts, attempts) = crate::run_pairing::pair_issuances(issuances, &report_refs);

	// An in-flight attempt is the more useful of the two to report, since it
	// says the pipeline is working right now.
	let newest_in_flight = attempts
		.iter()
		.any(|a| matches!(a.status(now), crate::run_pairing::RunStatus::InProgress));
	if newest_in_flight {
		return Ok(Some(AttemptState::InFlight));
	}
	if attempts.is_empty() {
		return Ok(None);
	}
	Ok(Some(AttemptState::EndedWithoutReport))
}
