use commons_errors::{AppError, Result};
use commons_types::{
	geo::GeoPoint,
	server::{RESERVED_TAG_PREFIX, TagMap, kind::ServerKind, product::Product, rank::ServerRank},
	status::ShortStatus,
};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use jiff::{SignedDuration, Timestamp};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::pg_duration::PgDuration;
use super::url_field::UrlField;

/// How long a restore window stays open once an operator allows restores for a
/// server. Restores read the group's backup repo, so the window is deliberately
/// short-lived; opening it again re-arms it from the moment of the new request.
const RESTORE_WINDOW: SignedDuration = SignedDuration::from_hours(24);

/// Recompute each distinct, present group id, deduping repeats and skipping
/// `None`. Used by the server write paths that can change a group's canonical
/// member (membership/rank/kind/delete).
async fn recompute_groups(
	db: &mut AsyncPgConnection,
	groups: impl IntoIterator<Item = Option<Uuid>>,
) -> Result<()> {
	let mut seen: Vec<Uuid> = Vec::new();
	for gid in groups.into_iter().flatten() {
		if !seen.contains(&gid) {
			seen.push(gid);
			crate::server_groups::ServerGroup::recompute_version(db, gid).await?;
		}
	}
	Ok(())
}

/// A single server in the fleet: the unit that reports status, files
/// issues, and is monitored for reachability. A server may belong to a
/// server group, and may or may not have a device enrolled against it yet.
#[derive(
	Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable, utoipa::ToSchema,
)]
#[diesel(table_name = crate::schema::applications)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Application {
	/// Unique identifier for this server.
	pub id: Uuid,
	/// The server's display name, scoped within its group. May not be
	/// globally unique or meaningful outside the group.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub name: Option<String>,

	/// The server's URL. Optional: a server may be identified solely by its
	/// enrolled device (e.g. a Tailscale node) rather than a URL. Not
	/// required to be unique. Responses that need a value to display even
	/// when this is unset fall back to another identifying field.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub host: Option<UrlField>,

	/// The application this server runs, for example tamanu or senaite.
	/// Decides which of canopy's per-server features apply to it at all.
	// spec: APP#product-and-kind
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub product: Product,
	/// The server's role within its product's topology, for example central
	/// or facility. Which roles are available depends on the product.
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub kind: ServerKind,
	/// The server's environment tier, for example production, test, or dev.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub rank: Option<ServerRank>,
	/// The device enrolled against this server, if any.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub device_id: Option<Uuid>,
	/// The machine this application runs on. An application runs on exactly
	/// one; a machine hosts any number.
	// spec: FLT#cardinality
	pub machine_id: Uuid,
	/// The server group this server belongs to, if any.
	///
	/// A denormalisation of the machine's group: an operator sets the group on
	/// the machine and the applications on it take it. Carried here so a group
	/// query reads one column rather than joining through the machine.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub group_id: Option<Uuid>,
	/// If set, the server is listed publicly under this name (used by
	/// end-user-facing clients). `None` means it is not listed publicly.
	/// Separate from `name` because that field is only meaningful within
	/// the server's group.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub public_name: Option<String>,
	/// Whether this server is hosted in the cloud, if known.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub cloud: Option<bool>,
	/// The server's physical location, if known.
	#[serde(skip_serializing_if = "Option::is_none")]
	pub geolocation: Option<GeoPoint>,
	/// Whether this server is actively monitored. When `false`, reachability
	/// checks skip it entirely and its issues no longer contribute to
	/// incidents — useful for test environments and ad-hoc demos. The
	/// `alert_when_down_for` threshold is preserved while unmonitored, so
	/// turning monitoring back on doesn't lose the configured value.
	pub is_monitored: bool,
	/// How long, in seconds, a server's status may go without an update
	/// before it's considered unreachable and an issue is filed. Increase
	/// it for applications with flaky connectivity; decrease it for critical
	/// applications that should alert promptly. Only enforced while
	/// `is_monitored` is `true`. Must be a positive number of seconds;
	/// defaults to 600 (10 minutes) for newly-created applications.
	#[schema(value_type = i64)]
	pub alert_when_down_for: PgDuration,
	/// Free-form operator notes about this server.
	#[serde(default)]
	pub notes: String,
	/// Key/value tags for this server.
	#[serde(default)]
	pub tags: TagMap,
	/// When set, the server is archived: hidden from live listings and
	/// monitoring, with its device unenrolled, but its history is retained.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub deleted_at: Option<Timestamp>,
	/// When a device successfully completed enrollment for this server.
	/// While `None`, the server is awaiting its first check-in and the UI
	/// shows setup instructions.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub registered_at: Option<Timestamp>,
	/// Until when this server is allowed to mint restore credentials for
	/// itself (ad-hoc `bestool canopy restore`). An operator opens this
	/// window and it auto-expires; `None` (or a past instant) means restores
	/// are not currently allowed. Restores read the group's backup repo, so
	/// they're gated behind this deliberate, time-boxed opt-in rather than
	/// always available.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub restore_allowed_until: Option<Timestamp>,
	/// Who opened the current restore window (Tailscale login), if any.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub restore_allowed_by: Option<String>,
	/// Whether this server may manage its own DNS records for names under its
	/// group's domains. Withheld by default: a server without it is
	/// authenticated and refused. Unlike the restore window this is a standing
	/// grant, records needing maintenance for as long as the server lives.
	// spec: DOM#permission-for-a-server-to-manage-its-own-names
	pub may_manage_dns: bool,
	/// Whether this server may obtain TLS certificates for names under its
	/// group's domains. Separate from `may_manage_dns`: an application whose
	/// records are managed elsewhere may still want its certificates here.
	// spec: DOM#permission-for-a-server-to-manage-its-own-names
	pub may_manage_tls: bool,
	/// The certificate profile — the authority's name for a lifetime — this
	/// server's certificates are requested under. `None` means the longest the
	/// authority offers, which is every server until an operator says otherwise:
	/// a short lifetime is adopted deliberately rather than inherited.
	// spec: CRT#lifetime
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub certificate_profile: Option<String>,
	/// When this server's name management was paused. While set, Canopy makes no
	/// new changes on its behalf — nothing ordered, renewed, or republished —
	/// though nothing already in place is withdrawn.
	///
	/// Set automatically when one of the server's certificates is revoked, so
	/// revocation and re-issuance don't chase each other. Only an operator lifts
	/// it.
	// spec: CRT#pausing-a-server
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(
		deserialize_as = jiff_diesel::NullableTimestamp,
		serialize_as = jiff_diesel::NullableTimestamp,
		treat_none_as_default_value = false
	)]
	pub name_management_paused_at: Option<Timestamp>,
	/// Who paused it. `None` when Canopy paused it itself on a revocation.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub name_management_paused_by: Option<String>,
	/// Why it was paused.
	#[serde(skip_serializing_if = "Option::is_none")]
	#[diesel(treat_none_as_default_value = false)]
	pub name_management_pause_reason: Option<String>,
}

impl Application {
	pub async fn get_all(
		db: &mut AsyncPgConnection,
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		let q = applications
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.order_by((
				name.is_not_null(),
				kind.asc(),
				name.asc(),
				created_at.desc(),
			))
			.offset(offset.try_into().unwrap_or(i64::MAX));

		if let Some(limit) = limit {
			q.limit(limit.try_into().unwrap_or(i64::MAX)).load(db).await
		} else {
			q.load(db).await
		}
		.map_err(AppError::from)
	}

	/// Archived (soft-deleted) applications, for the Archived view.
	pub async fn list_archived(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_not_null())
			.order_by((kind.asc(), name.asc(), created_at.desc()))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn list_by_kind(
		db: &mut AsyncPgConnection,
		k: ServerKind,
		offset: u64,
		limit: Option<u64>,
	) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		let q = applications
			.select(Self::as_select())
			.filter(id.ne(Uuid::nil()).and(kind.eq(k)))
			.filter(deleted_at.is_null())
			.order_by((name.is_not_null(), name.asc(), created_at.desc()))
			.offset(offset.try_into().unwrap_or(i64::MAX));

		if let Some(limit) = limit {
			q.limit(limit.try_into().unwrap_or(i64::MAX)).load(db).await
		} else {
			q.load(db).await
		}
		.map_err(AppError::from)
	}

	pub async fn count_all(db: &mut AsyncPgConnection) -> Result<u64> {
		use crate::schema::applications::dsl::*;
		applications
			.count()
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	pub async fn count_by_kind(db: &mut AsyncPgConnection, k: ServerKind) -> Result<u64> {
		use crate::schema::applications::dsl::*;
		applications
			.count()
			.filter(id.ne(Uuid::nil()).and(kind.eq(k)))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	pub async fn get_by_id(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::schema::applications::table
			.select(Self::as_select())
			.filter(crate::schema::applications::id.eq(id))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Like [`Application::get_by_id`] but takes a `FOR UPDATE` row lock. A caller
	/// inside a transaction uses this to serialise against concurrent archival
	/// (`soft_delete` locks the same row), closing the archive-vs-register
	/// TOCTOU at enrollment completion.
	pub async fn get_by_id_for_update(db: &mut AsyncPgConnection, id: Uuid) -> Result<Self> {
		crate::schema::applications::table
			.select(Self::as_select())
			.filter(crate::schema::applications::id.eq(id))
			.for_update()
			.first(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_host(db: &mut AsyncPgConnection, host: String) -> Result<Self> {
		crate::schema::applications::table
			.select(Self::as_select())
			.filter(crate::schema::applications::host.eq(host))
			.first(db)
			.await
			.map_err(AppError::from)
	}

	/// Operator-driven insert. The caller pre-builds the row (id, defaults,
	/// optional URL, optional pre-bound `device_id` for the Tailscale case).
	/// URLs are no longer unique, so there is no collision check.
	pub async fn create(db: &mut AsyncPgConnection, server: Application) -> Result<Self> {
		use crate::schema::applications;

		crate::tags::reject_reserved_keys(&server.tags)?;

		let created = diesel::insert_into(applications::table)
			.values(server)
			.returning(Self::as_select())
			.get_result(db)
			.await
			.map_err(AppError::from)?;
		// A new member can change the group's canonical version source.
		recompute_groups(db, [created.group_id]).await?;
		Ok(created)
	}

	/// Archive a server: hide it from live listings and monitoring while
	/// retaining its history. Releases its device (clears `device_id` and
	/// revokes the device's credentials) so the box can only return through the
	/// gated enrollment flow. Idempotent.
	pub async fn soft_delete(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::applications::dsl;
		use diesel_async::AsyncConnection;

		db.transaction::<_, AppError, _>(async |conn| {
			let server: Application = dsl::applications
				.select(Self::as_select())
				.filter(dsl::id.eq(server_id))
				.for_update()
				.first(conn)
				.await
				.map_err(AppError::from)?;

			if server.deleted_at.is_some() {
				return Ok(());
			}

			if let Some(device_id) = server.device_id {
				crate::devices::Device::revoke(conn, device_id).await?;
			}

			diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
				.set((
					dsl::deleted_at
						.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
					dsl::registered_at.eq(None::<jiff_diesel::Timestamp>),
					dsl::device_id.eq(None::<Uuid>),
				))
				.execute(conn)
				.await
				.map_err(AppError::from)?;

			// The server just dropped out of its group's live set, so the
			// group's cached headline version may now belong to someone else.
			recompute_groups(conn, [server.group_id]).await?;
			Ok(())
		})
		.await
	}

	/// Un-archive a server. Does not rebind a device — the box must re-enroll.
	pub async fn restore(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<Self> {
		use crate::schema::applications::dsl;

		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set(dsl::deleted_at.eq(None::<jiff_diesel::Timestamp>))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		let restored = Self::get_by_id(db, server_id).await?;
		// Back in the live set: the group's canonical member may change.
		recompute_groups(db, [restored.group_id]).await?;
		Ok(restored)
	}

	/// Canonicalise a user-entered URL. A bare host (no scheme) defaults to
	/// `https://`, so operators can type `foo.example.com` and get
	/// `https://foo.example.com`.
	pub fn canonicalize_host(url: &str) -> Result<UrlField> {
		let url = url.trim();
		let candidate = if url.contains("://") {
			url.to_string()
		} else {
			format!("https://{url}")
		};
		Ok(UrlField(candidate.parse().map_err(|e| {
			AppError::BadRequest(format!("Invalid URL: {e}"))
		})?))
	}

	/// Map a hosting hint to the `cloud` flag.
	pub fn detect_cloud(hosting: &str) -> bool {
		matches!(
			hosting,
			"ec2" | "azure" | "gce" | "gcp" | "digitalocean" | "oracle" | "cloudstack"
		)
	}

	/// Live (non-archived) applications currently bound to this device.
	pub async fn live_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
			.filter(deleted_at.is_null())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// Bind a device to a server (sets `device_id`).
	pub async fn bind_device(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		device_id: Uuid,
	) -> Result<()> {
		use crate::schema::applications::dsl;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set(dsl::device_id.eq(Some(device_id)))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Mark a server enrolled (sets `registered_at = now()`).
	pub async fn mark_registered(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::applications::dsl;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set(
				dsl::registered_at.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
			)
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Open the restore window for a server: allow it to mint restore
	/// credentials for itself until [`RESTORE_WINDOW`] from now. Re-arming an
	/// already-open window resets the expiry. Returns the new expiry so callers
	/// can echo it back to the operator. `allowed_by` is the operator's identity
	/// (Tailscale login) for audit.
	/// Pause Canopy acting on this server's behalf: no certificate ordered or
	/// renewed, no address record changed. Nothing already in place is withdrawn.
	///
	/// Pausing an already-paused server leaves the original pause standing, so the
	/// recorded reason and age stay those of the pause that first stopped the
	/// work — which is the one an operator is investigating.
	/// This application's reachability, from its latest report and its own down
	/// threshold.
	///
	/// The threshold lives here rather than at each call site so the indicator
	/// and the `reachability` check cannot be graded on different clocks, which
	/// is exactly how they came to disagree.
	// spec: CHK#reachability
	pub fn reachability(&self, latest: Option<&crate::statuses::Status>) -> ShortStatus {
		latest.map_or(ShortStatus::Gone, |status| {
			status.short_status(self.alert_when_down_for.0)
		})
	}

	// spec: CRT#pausing-a-server
	/// Whether Canopy is currently making no new changes on this server's behalf.
	pub fn name_management_paused(&self) -> bool {
		self.name_management_paused_at.is_some()
	}

	pub async fn pause_name_management(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		paused_by: Option<&str>,
		reason: &str,
	) -> Result<()> {
		use crate::schema::applications::dsl;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.filter(dsl::name_management_paused_at.is_null())
			.set((
				dsl::name_management_paused_at
					.eq(jiff_diesel::NullableTimestamp::from(Some(Timestamp::now()))),
				dsl::name_management_paused_by.eq(paused_by),
				dsl::name_management_pause_reason.eq(Some(reason)),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Lift a pause, so the work resumes where it left off.
	///
	/// Only ever called for an operator: Canopy pauses itself on a revocation but
	/// never un-pauses itself, however long the pause has stood and however much
	/// is expiring under it. Deciding it is safe to start again is a judgement
	/// Canopy is not in a position to make.
	// spec: CRT#pausing-a-server
	pub async fn resume_name_management(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::applications::dsl;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set((
				dsl::name_management_paused_at.eq(jiff_diesel::NullableTimestamp::from(None)),
				dsl::name_management_paused_by.eq::<Option<String>>(None),
				dsl::name_management_pause_reason.eq::<Option<String>>(None),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Set (or clear) the profile this server's certificates are requested under.
	///
	/// `None` means the authority's own default, which is its longest-lived: a
	/// short lifetime is something adopted deliberately for a server rather than a
	/// default anyone inherits. Takes effect on the next issuance or renewal; a
	/// certificate already held keeps the lifetime it was issued with.
	// spec: CRT#lifetime
	pub async fn set_certificate_profile(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		profile: Option<&str>,
	) -> Result<()> {
		use crate::schema::applications::dsl;
		let updated = diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.filter(dsl::deleted_at.is_null())
			.set(dsl::certificate_profile.eq(profile))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		if updated == 0 {
			return Err(AppError::DatabaseQuery(diesel::result::Error::NotFound));
		}
		Ok(())
	}

	pub async fn allow_restore(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		allowed_by: Option<&str>,
	) -> Result<Timestamp> {
		use crate::schema::applications::dsl;
		let until = Timestamp::now() + RESTORE_WINDOW;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set((
				dsl::restore_allowed_until.eq(jiff_diesel::NullableTimestamp::from(Some(until))),
				dsl::restore_allowed_by.eq(allowed_by),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(until)
	}

	/// Close the restore window for a server immediately (clears both the
	/// expiry and the recorded operator). Clearing an already-closed window is a
	/// no-op.
	pub async fn disallow_restore(db: &mut AsyncPgConnection, server_id: Uuid) -> Result<()> {
		use crate::schema::applications::dsl;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set((
				dsl::restore_allowed_until.eq(jiff_diesel::NullableTimestamp::from(None)),
				dsl::restore_allowed_by.eq::<Option<String>>(None),
			))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		Ok(())
	}

	/// Whether this server's restore window is currently open (set and not yet
	/// expired).
	pub fn restore_allowed(&self) -> bool {
		self.restore_allowed_until
			.is_some_and(|until| until > Timestamp::now())
	}

	pub async fn get_by_device_id(db: &mut AsyncPgConnection, dev_id: Uuid) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(device_id.eq(dev_id))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn get_by_ids(db: &mut AsyncPgConnection, ids: &[Uuid]) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All applications in the same group as `self`, excluding `self`. If the
	/// server is ungrouped, returns an empty Vec.
	pub async fn siblings(&self, db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		let Some(gid) = self.group_id else {
			return Ok(Vec::new());
		};
		applications
			.select(Self::as_select())
			.filter(group_id.eq(gid))
			.filter(id.ne(self.id))
			.filter(deleted_at.is_null())
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All live (non-archived) applications in a group, ordered by name. Used to
	/// expand a group-wide restore-replica declaration into per-server entries.
	pub async fn list_live_in_group(
		db: &mut AsyncPgConnection,
		group_id_: Uuid,
	) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(group_id.eq(group_id_))
			.filter(deleted_at.is_null())
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	/// All applications without a group, ordered by name. Used by the Ungrouped UI tab.
	pub async fn list_ungrouped(db: &mut AsyncPgConnection) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		applications
			.select(Self::as_select())
			.filter(group_id.is_null())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.order(name.asc())
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn count_ungrouped(db: &mut AsyncPgConnection) -> Result<u64> {
		use crate::schema::applications::dsl::*;
		applications
			.count()
			.filter(group_id.is_null())
			.filter(id.ne(Uuid::nil()))
			.filter(deleted_at.is_null())
			.get_result(db)
			.await
			.map_err(AppError::from)
			.map(|n: i64| n.try_into().unwrap_or_default())
	}

	/// Bulk-fetch `(name, host)` for a set of server ids — used by the
	/// issues/incidents APIs to embed display info into each row so the UI
	/// doesn't have to fetch every server independently.
	pub async fn names_by_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, (Option<String>, Option<String>)>> {
		use crate::schema::applications::dsl;

		if ids.is_empty() {
			return Ok(std::collections::HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>, Option<String>)> = dsl::applications
			.select((dsl::id, dsl::name, dsl::host))
			.filter(dsl::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().map(|(i, n, h)| (i, (n, h))).collect())
	}

	/// Bulk-fetch the group name for each given server. Servers that are
	/// ungrouped (or don't exist) get `None`.
	pub async fn group_names_by_server_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, Option<String>>> {
		use crate::schema::{applications, server_groups};
		use std::collections::HashMap;

		if ids.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<(Uuid, Option<String>)> = applications::table
			.left_join(
				server_groups::table.on(server_groups::id.nullable().eq(applications::group_id)),
			)
			.select((applications::id, server_groups::name.nullable()))
			.filter(applications::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows.into_iter().collect())
	}

	/// Bulk-fetch `(group_id, group_name)` for each given server. Servers
	/// that are ungrouped (or don't exist) get `(None, None)`.
	pub async fn group_refs_by_server_ids(
		db: &mut AsyncPgConnection,
		ids: &[Uuid],
	) -> Result<std::collections::HashMap<Uuid, (Option<Uuid>, Option<String>)>> {
		use crate::schema::{applications, server_groups};
		use std::collections::HashMap;

		if ids.is_empty() {
			return Ok(HashMap::new());
		}
		let rows: Vec<(Uuid, Option<Uuid>, Option<String>)> = applications::table
			.left_join(
				server_groups::table.on(server_groups::id.nullable().eq(applications::group_id)),
			)
			.select((
				applications::id,
				applications::group_id,
				server_groups::name.nullable(),
			))
			.filter(applications::id.eq_any(ids))
			.load(db)
			.await
			.map_err(AppError::from)?;
		Ok(rows
			.into_iter()
			.map(|(id, gid, gn)| (id, (gid, gn)))
			.collect())
	}

	pub async fn search_central(
		db: &mut AsyncPgConnection,
		query: &str,
		limit: i64,
	) -> Result<Vec<Self>> {
		use crate::schema::applications::dsl::*;
		let search_pattern = format!("%{}%", query);

		// Both halves of eligibility, stated rather than implied: only a
		// product canopy lists publicly, and only its central applications. The
		// kind filter alone would exclude other products today, but by
		// accident of their having no central role rather than on purpose.
		// spec: APP#public-listing
		let mut query_builder = applications
			.select(Self::as_select())
			.filter(product.eq_any(Product::stored_values_where(|p| p.caps().public_listing)))
			.filter(kind.eq(ServerKind::Central.to_string()))
			.filter(public_name.is_not_null())
			.filter(deleted_at.is_null())
			.into_boxed();

		if let Ok(query_uuid) = query.parse::<Uuid>() {
			query_builder = query_builder.filter(
				name.ilike(&search_pattern)
					.or(host.ilike(&search_pattern))
					.or(id.eq(query_uuid)),
			);
		} else {
			query_builder =
				query_builder.filter(name.ilike(&search_pattern).or(host.ilike(&search_pattern)));
		}

		query_builder
			.limit(limit)
			.load(db)
			.await
			.map_err(AppError::from)
	}

	pub async fn update(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		updates: PartialServer,
	) -> Result<Self> {
		use crate::schema::applications::dsl;

		if let Some(tags) = &updates.tags {
			crate::tags::reject_reserved_keys(tags)?;
		}

		// Capture the old group before the update: rank/kind/group_id may all
		// change, so both the old and new group's canonical member can shift.
		// Non-fatal: a missing server (or read error) just means "no old group
		// to recompute" and leaves the update's own error handling — e.g. the
		// empty-changeset path — to set the response, unchanged by us.
		let old_group_id = Self::get_by_id(db, server_id)
			.await
			.ok()
			.and_then(|s| s.group_id);

		// An empty changeset is a no-op, not an error. It became reachable when
		// the group moved to the machine: an edit that changes only the group
		// leaves nothing here to write, and diesel refuses to build that query.
		match diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set(updates)
			.execute(db)
			.await
		{
			Ok(_) => {}
			Err(diesel::result::Error::QueryBuilderError(_)) => {}
			Err(err) => return Err(AppError::from(err)),
		}

		let after = Self::get_by_id(db, server_id).await?;
		recompute_groups(db, [old_group_id, after.group_id]).await?;
		Ok(after)
	}

	/// Set or clear the server's group. On a `None → Some(group)` transition,
	/// the server's currently-open issues get re-evaluated against the new
	/// group so any that warrant promotion to an incident do so. The clear
	/// case is the simpler direction: the server's open issues stay, but
	/// they no longer contribute to a group-level incident (the existing
	/// incident's other-server contributors keep it alive on their own).
	pub async fn assign_to_group(
		db: &mut AsyncPgConnection,
		server_id: Uuid,
		new_group_id: Option<Uuid>,
	) -> Result<Self> {
		use crate::schema::applications::dsl;

		let before = Self::get_by_id(db, server_id).await?;
		diesel::update(dsl::applications.filter(dsl::id.eq(server_id)))
			.set(dsl::group_id.eq(new_group_id))
			.execute(db)
			.await
			.map_err(AppError::from)?;
		let after = Self::get_by_id(db, server_id).await?;

		if before.group_id.is_none() && new_group_id.is_some() {
			// Promote currently-open issues now that this server has somewhere
			// to attach an incident to.
			crate::issues::reevaluate_open_issues_for_server(db, server_id).await?;
		}

		recompute_groups(db, [before.group_id, new_group_id]).await?;

		Ok(after)
	}

	/// Tags as seen by the public-server tags endpoint: the group's tags
	/// with the server's own tags overlaid (server wins on key collision).
	/// If the server is ungrouped, returns just the server's tags.
	pub async fn tags_merged_with_group(&self, db: &mut AsyncPgConnection) -> Result<TagMap> {
		let Some(gid) = self.group_id else {
			return Ok(self.tags.clone());
		};
		let group = crate::server_groups::ServerGroup::get_by_id(db, gid).await?;
		Ok(self.tags.merged_with(&group.tags))
	}

	/// Tags served to the device by the public `/tags` endpoint: the merged
	/// server+group tags (see [`Self::tags_merged_with_group`]) plus synthetic
	/// read-only tags describing the server itself, under the reserved
	/// [`RESERVED_TAG_PREFIX`] namespace:
	///
	/// - `canopy:product` — the server's [`Product`] (always present).
	/// - `canopy:kind` — the server's [`ServerKind`] (always present).
	/// - `canopy:rank` — the server's [`ServerRank`], only when one is set.
	/// - `canopy:group-id` / `canopy:group-name` — only when grouped.
	///
	/// Operator-set tags can't use the `canopy:` prefix (rejected on write),
	/// so these never collide with stored tags.
	pub async fn tags_for_device(&self, db: &mut AsyncPgConnection) -> Result<TagMap> {
		let group = match self.group_id {
			Some(gid) => Some(crate::server_groups::ServerGroup::get_by_id(db, gid).await?),
			None => None,
		};

		let mut tags = match &group {
			Some(group) => self.tags.merged_with(&group.tags),
			None => self.tags.clone(),
		};

		tags.0.insert(
			format!("{RESERVED_TAG_PREFIX}product"),
			self.product.to_string(),
		);
		tags.0
			.insert(format!("{RESERVED_TAG_PREFIX}kind"), self.kind.to_string());
		if let Some(rank) = self.rank {
			tags.0
				.insert(format!("{RESERVED_TAG_PREFIX}rank"), rank.to_string());
		}
		if let Some(group) = &group {
			tags.0.insert(
				format!("{RESERVED_TAG_PREFIX}group-id"),
				group.id.to_string(),
			);
			tags.0.insert(
				format!("{RESERVED_TAG_PREFIX}group-name"),
				group.name.clone(),
			);
		}

		Ok(tags)
	}
}

#[test]
fn canonicalize_host_defaults_to_https() {
	let h = |s: &str| Application::canonicalize_host(s).unwrap().0.to_string();
	assert_eq!(h("foo.example.com"), "https://foo.example.com/");
	assert_eq!(h("  bar.example.com  "), "https://bar.example.com/");
	assert_eq!(h("http://insecure.example"), "http://insecure.example/");
	assert_eq!(h("https://full.example/path"), "https://full.example/path");
}

#[test]
fn test_server_serialization() {
	let server = Application {
		id: Uuid::nil(),
		name: Some("Test Application".to_string()),
		product: Product::Tamanu,
		kind: ServerKind::Central,
		rank: Some(ServerRank::Production),
		host: Some(UrlField("https://example.com/".parse().unwrap())),
		device_id: Some(Uuid::nil()),
		machine_id: Uuid::nil(),
		group_id: None,
		public_name: Some("Test Application".to_string()),
		cloud: None,
		geolocation: None,
		is_monitored: true,
		alert_when_down_for: PgDuration(SignedDuration::from_secs(600)),
		notes: String::new(),
		tags: TagMap::default(),
		deleted_at: None,
		registered_at: None,
		restore_allowed_until: None,
		restore_allowed_by: None,
		may_manage_dns: false,
		may_manage_tls: false,
		certificate_profile: None,
		name_management_paused_at: None,
		name_management_paused_by: None,
		name_management_pause_reason: None,
	};

	let serialized = serde_json::to_string_pretty(&server).unwrap();
	assert_eq!(
		serialized,
		r#"{
  "id": "00000000-0000-0000-0000-000000000000",
  "name": "Test Application",
  "host": "https://example.com",
  "product": "tamanu",
  "kind": "central",
  "rank": "production",
  "device_id": "00000000-0000-0000-0000-000000000000",
  "machine_id": "00000000-0000-0000-0000-000000000000",
  "public_name": "Test Application",
  "is_monitored": true,
  "alert_when_down_for": 600,
  "notes": "",
  "tags": {},
  "may_manage_dns": false,
  "may_manage_tls": false
}"#
	);
}

/// Fields to update on an existing server. Only the fields present are
/// changed; omitted fields are left as-is. For the fields that are
/// themselves optional (`host`, `device_id`, `group_id`, `public_name`,
/// `cloud`, `geolocation`), sending an explicit `null` clears the value,
/// while omitting the field entirely leaves it unchanged.
#[derive(Debug, Deserialize, AsChangeset, utoipa::ToSchema)]
#[diesel(table_name = crate::schema::applications)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct PartialServer {
	/// The server to update.
	pub id: Uuid,
	/// New display name for the server.
	pub name: Option<String>,
	/// New application for the server. When this changes to a product that
	/// does not define the server's current kind, the caller settles the kind
	/// too — the endpoint moves it to the new product's default.
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub product: Option<Product>,
	/// New role for the server, for example central or facility.
	pub kind: Option<ServerKind>,
	/// New environment tier for the server, for example production, test,
	/// or dev.
	#[diesel(deserialize_as = String, serialize_as = String)]
	pub rank: Option<ServerRank>,
	/// New URL for the server, or `null` to clear it.
	pub host: Option<Option<UrlField>>,
	/// New enrolled device for the server, or `null` to unenroll it.
	pub device_id: Option<Option<Uuid>>,
	/// New server group for the server, or `null` to remove it from its
	/// current group.
	pub group_id: Option<Option<Uuid>>,
	/// New public listing name for the server, or `null` to unlist it.
	pub public_name: Option<Option<String>>,
	/// New value for whether the server is cloud-hosted, or `null` to clear
	/// it.
	pub cloud: Option<Option<bool>>,
	/// New physical location for the server, or `null` to clear it.
	pub geolocation: Option<Option<GeoPoint>>,
	/// New monitored state for the server.
	pub is_monitored: Option<bool>,
	/// New downtime threshold for the server, in seconds.
	#[schema(value_type = Option<i64>)]
	#[diesel(serialize_as = PgDuration)]
	pub alert_when_down_for: Option<PgDuration>,
	/// New free-form operator notes for the server.
	pub notes: Option<String>,
	/// New set of key/value tags for the server. This replaces the whole
	/// tag set.
	pub tags: Option<TagMap>,
	/// Whether the server may manage its own DNS records under its group's
	/// domains.
	pub may_manage_dns: Option<bool>,
	/// Whether the server may obtain TLS certificates for names under its
	/// group's domains.
	pub may_manage_tls: Option<bool>,
}
