//! Operator endpoints for machines: the hosts applications run on.
//!
//! A machine is what an operator creates and places in a group; the
//! applications on it arrive by report and take the machine's group. So the
//! writes here are the group, the box's location, and its monitoring
//! settings — never an application's type or version.
//!
//! Identity is deliberately absent: an identity is bound by enrolment, not by
//! an operator editing a form.

use axum::Json;
use axum::extract::State;
use canopy_utoipa_axum::{router::OpenApiRouter, routes};
use commons_errors::{AppError, ProblemDetailsSchema, Result};
use commons_servers::tailscale_auth::TailscaleAdmin;
use commons_types::{Uuid, device::DeviceRole, geo::GeoPoint, server::TagMap};
use database::applications::Application;
use database::devices::{Device, TailscaleIdentity};
use database::machines::{Machine, MachineUpdate, NewMachine};
use database::pg_duration::PgDuration;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::state::AppState;

pub fn routes() -> OpenApiRouter<AppState> {
	OpenApiRouter::new()
		.routes(routes!(list))
		.routes(routes!(get))
		.routes(routes!(create))
		.routes(routes!(update))
		.routes(routes!(archive))
}

/// Identifies one machine.
#[derive(Deserialize, ToSchema)]
pub struct MachineIdArgs {
	/// The machine to operate on.
	pub machine_id: Uuid,
}

/// A machine together with the applications running on it.
#[derive(Serialize, ToSchema)]
pub struct MachineDetail {
	#[serde(flatten)]
	pub machine: Machine,
	/// The applications on this machine. Empty for a machine that has been
	/// created but has not yet reported: that is awaiting check-in, not an
	/// error.
	pub applications: Vec<Application>,
}

/// List the live fleet's machines.
///
/// Every machine that has not been archived, ordered by name. A machine with
/// no applications on it is included: one created but not yet reporting is
/// awaiting check-in, not an error. The request body is ignored; send an empty
/// JSON object.
#[utoipa::path(
	post,
	path = "/list",
	operation_id = "machines_list",
	tag = "machines",
	security(("tailscale-user" = [])),
	responses(
		(status = 200, body = Vec<Machine>),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn list(State(state): State<AppState>) -> Result<Json<Vec<Machine>>> {
	let mut conn = state.db_read.get().await?;
	Ok(Json(Machine::list_live(&mut conn).await?))
}

/// Get one machine and the applications running on it.
///
/// Returns the machine's own facts — its group, where it is, how long it may
/// be silent — together with the applications it hosts. Returns 404 if the
/// machine doesn't exist.
#[utoipa::path(
	post,
	path = "/get",
	operation_id = "machines_get",
	tag = "machines",
	security(("tailscale-user" = [])),
	request_body = MachineIdArgs,
	responses(
		(status = 200, body = MachineDetail),
		(status = 404, body = ProblemDetailsSchema),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn get(
	State(state): State<AppState>,
	Json(args): Json<MachineIdArgs>,
) -> Result<Json<MachineDetail>> {
	let mut conn = state.db_read.get().await?;
	let machine = Machine::get_by_id(&mut conn, args.machine_id).await?;
	let applications = machine.applications(&mut conn).await?;
	Ok(Json(MachineDetail {
		machine,
		applications,
	}))
}

/// What an operator supplies when adding a machine.
///
/// The group is the only field that matters to get right up front: which
/// group a box belongs to is the one thing the box has no way of knowing.
#[derive(Deserialize, ToSchema)]
pub struct MachineCreateArgs {
	/// What to call the box. Distinct from the hostname its operating system
	/// reports, which arrives as a reported figure.
	pub name: Option<String>,
	/// The group this machine belongs to. The applications on it take it.
	pub group_id: Option<Uuid>,
	/// Whether the box is cloud-hosted, if known.
	pub cloud: Option<bool>,
	/// Where the box is, if known.
	pub geolocation: Option<GeoPoint>,
	/// Whether the machine's own checks alert. Defaults to on.
	pub is_monitored: Option<bool>,
	/// How long the box may be silent before it is unreachable, in seconds.
	/// Defaults to the column's own default.
	pub alert_when_down_for: Option<i64>,
	/// Operator notes shown on the machine's page.
	pub notes: Option<String>,
	/// Operator-set tags. Reserved names are refused, as they are on an edit.
	pub tags: Option<TagMap>,
	/// A tailnet node to bind this machine to up front, by address, node key,
	/// or DNS name. Omit to enrol the machine later.
	///
	/// The operator is describing a box they can already see on the tailnet,
	/// so binding it here saves an enrolment round trip. The identity is a
	/// machine's, not an application's: the applications on the box are
	/// reported and carry no identity of their own.
	pub tailscale_identifier: Option<String>,
}

/// Add a machine to the fleet.
///
/// The machine starts with no applications and no identity; enrolment binds an
/// identity, and the applications on it arrive by report.
#[utoipa::path(
	post,
	path = "/create",
	operation_id = "machines_create",
	tag = "machines",
	security(("tailscale-user" = [])),
	request_body = MachineCreateArgs,
	responses(
		(status = 200, body = Uuid),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn create(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<MachineCreateArgs>,
) -> Result<Json<Uuid>> {
	let mut conn = state.db.get().await?;

	// Bind the tailnet node first: a machine created and then failed to bind
	// would leave a record the operator did not ask for.
	let device_id = match args.tailscale_identifier.as_deref() {
		None => None,
		Some(identifier) => {
			let directory = state
				.tailnet_directory
				.as_ref()
				.ok_or(AppError::AuthTailnetDirectoryUnavailable)?;
			let entry = directory
				.resolve_identifier(identifier)
				.await
				.map_err(|_| AppError::AuthTailnetDirectoryUnavailable)?
				.ok_or_else(|| {
					AppError::NotFound("no tailnet device matches that identifier".into())
				})?;
			let device = match Device::from_tailscale_node_id(&mut conn, &entry.node_id).await? {
				Some(existing) => existing,
				None => {
					Device::create_with_tailscale(
						&mut conn,
						TailscaleIdentity {
							node_id: entry.node_id.clone(),
							node_name: Some(entry.node_name.clone()),
							tailnet: Some(entry.tailnet.clone()),
						},
						DeviceRole::Server,
					)
					.await?
				}
			};
			Some(device.id)
		}
	};

	let machine = Machine::create(
		&mut conn,
		NewMachine {
			name: args.name,
			group_id: args.group_id,
			cloud: args.cloud,
			geolocation: args.geolocation,
		},
	)
	.await?;

	// The rest of the form is an edit applied to the machine just made, so
	// creating and editing cannot disagree about what a field means.
	Machine::update(
		&mut conn,
		machine.id,
		MachineUpdate {
			is_monitored: args.is_monitored,
			alert_when_down_for: args
				.alert_when_down_for
				.map(|s| PgDuration(jiff::SignedDuration::from_secs(s))),
			notes: args.notes,
			tags: args.tags,
			..Default::default()
		},
	)
	.await?;

	if let Some(device_id) = device_id {
		Machine::bind_device(&mut conn, machine.id, device_id).await?;
	}

	Ok(Json(machine.id))
}

/// Fields to change on a machine. Omitted fields are left alone; for the
/// nullable ones an explicit `null` clears the value.
#[derive(Deserialize, ToSchema)]
pub struct MachineUpdateArgs {
	/// The machine to edit.
	pub machine_id: Uuid,
	/// New name for the box, or `null` to clear it.
	pub name: Option<Option<String>>,
	/// New group, or `null` to remove it from its current one. The
	/// applications on this machine move with it.
	pub group_id: Option<Option<Uuid>>,
	/// New value for whether the box is cloud-hosted, or `null` to clear it.
	pub cloud: Option<Option<bool>>,
	/// New location for the box, or `null` to clear it.
	pub geolocation: Option<Option<GeoPoint>>,
	/// New monitored state. Switching it off quiets the machine's own checks
	/// and leaves the applications on it alone.
	pub is_monitored: Option<bool>,
	/// How long the machine may be silent before it is unreachable, in seconds.
	pub alert_when_down_for: Option<i64>,
	/// New free-form operator notes for the box.
	pub notes: Option<String>,
	/// New set of key/value tags. Replaces the whole set.
	pub tags: Option<TagMap>,
}

/// Edit a machine.
///
/// Moving a machine to another group moves the applications on it: an
/// application's group is never set independently of its machine's.
#[utoipa::path(
	post,
	path = "/update",
	operation_id = "machines_update",
	tag = "machines",
	security(("tailscale-user" = [])),
	request_body = MachineUpdateArgs,
	responses(
		(status = 200, body = Machine),
		(status = 400, body = ProblemDetailsSchema),
		(status = 404, body = ProblemDetailsSchema),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn update(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<MachineUpdateArgs>,
) -> Result<Json<Machine>> {
	let mut conn = state.db.get().await?;
	let updated = Machine::update(
		&mut conn,
		args.machine_id,
		MachineUpdate {
			name: args.name,
			group_id: args.group_id,
			cloud: args.cloud,
			geolocation: args.geolocation,
			is_monitored: args.is_monitored,
			alert_when_down_for: args.alert_when_down_for.map(|secs| {
				database::pg_duration::PgDuration(jiff::SignedDuration::from_secs(secs))
			}),
			notes: args.notes,
			tags: args.tags,
		},
	)
	.await?;
	Ok(Json(updated))
}

/// Archive a machine, and with it the applications on it.
///
/// A box going away takes its workloads with it. Archival is not deletion:
/// the records and their history remain.
#[utoipa::path(
	post,
	path = "/archive",
	operation_id = "machines_archive",
	tag = "machines",
	security(("tailscale-user" = [])),
	request_body = MachineIdArgs,
	responses(
		(status = 200, body = ()),
		(status = 404, body = ProblemDetailsSchema),
		(status = 500, body = ProblemDetailsSchema),
	),
)]
pub async fn archive(
	State(state): State<AppState>,
	_admin: TailscaleAdmin,
	Json(args): Json<MachineIdArgs>,
) -> Result<Json<()>> {
	let mut conn = state.db.get().await?;
	Machine::archive(&mut conn, args.machine_id).await?;
	Ok(Json(()))
}
