//! In-process kopia execution layer.
//!
//! The `backups` Deployment runs kopia directly as a subprocess for each due
//! group (the kopia binary ships in the image). This module holds the pure,
//! tested helpers (retention policy, snapshot-manifest parsing, `content stats`
//! parsing) plus the subprocess wrappers and the per-kind orchestration
//! (`run_init`, `run_maintenance`, `run_inspect`).
//!
//! These fns return typed Rust values; the caller ([`super::complete`]) writes
//! the results to the database directly.
//!
//! ## Credentials
//!
//! The pod runs as the shared `canopy-jobs` IRSA identity. For an op, Canopy
//! assumes the group's **maintenance** role and gives kopia's S3 backend its
//! credentials one of two ways, both carried by [`KopiaEnv`] (which also carries
//! `KOPIA_PASSWORD`):
//!
//! - **Short ops** (init, inspection, rotation) take the assumed role's static
//!   credentials via `AWS_*` env. These are ~1h-lived, fine for a short op.
//! - **Long maintenance** is routed through the bestool-kopia SigV4 re-signing
//!   proxy ([`super::creds_server::CredsServer::spawn_maintenance_proxy`]): kopia
//!   connects to a loopback endpoint with dummy keys (`proxy_endpoint` set) and
//!   the proxy re-signs each request with freshly-refreshed credentials, so the
//!   run isn't capped at the assumed-session lifetime (maintenance can exceed an
//!   hour). See the S3P spec.
//!
//! Either way the pod's own IRSA / container-creds env vars are scrubbed off the
//! kopia subprocess so they can't shadow the per-op credentials.

use std::collections::BTreeMap;
use std::process::Output;

use anyhow::{Context, Result, bail};
use commons_types::backup::MaintenanceKind;
use tokio::process::Command;

/// The kopia maintenance owner identity. kopia maintenance is owned by a single
/// `user@host` and `maintenance run` refuses unless the connected client
/// identity equals the owner — so every op connects with this fixed canopy
/// identity (via `--override-username`/`--override-hostname`) and `init` sets it
/// as the owner. Devices connect with their own identity, so they never become
/// owner.
pub const MAINTENANCE_USER: &str = "canopy";
pub const MAINTENANCE_HOST: &str = "canopy-maintenance";

// ===========================================================================
// Per-op AWS + repo env.
// ===========================================================================

/// The per-op environment applied to every kopia [`Command`]. kopia's S3 backend
/// (minio-go) reads its AWS creds from `AWS_*` env. For short ops these are the
/// group's assumed-role static creds; for long maintenance they're dummy keys and
/// the request is re-signed by the proxy (`proxy_endpoint`). `KOPIA_PASSWORD`
/// carries the repo passphrase.
#[derive(Debug, Clone)]
pub struct KopiaEnv {
	/// `AWS_ACCESS_KEY_ID` — the assumed role's static key for direct ops, or a
	/// dummy value when `proxy_endpoint` is set (the proxy holds the real creds).
	pub access_key_id: String,
	/// `AWS_SECRET_ACCESS_KEY` (real, or dummy under the proxy).
	pub secret_access_key: String,
	/// `AWS_SESSION_TOKEN`. Empty under the proxy (dummy creds aren't STS); set
	/// for direct assumed-role creds. An empty value is not exported.
	pub session_token: String,
	/// The group's region (`AWS_REGION`), if set.
	pub region: Option<String>,
	/// The repo passphrase, read from the group's k8s Secret (`KOPIA_PASSWORD`).
	pub password: String,
	/// When set, kopia's S3 backend is pointed at this loopback proxy endpoint
	/// (`host:port`) with TLS disabled, and the credentials above are meaningless
	/// dummies — the [SigV4 re-signing proxy](super::creds_server) holds and
	/// refreshes the real credentials. When `None`, kopia talks to S3 directly
	/// with the static creds above.
	pub proxy_endpoint: Option<String>,
}

impl KopiaEnv {
	/// Apply the per-op env to a `kopia` Command. kopia 0.23.1's `s3` connector
	/// requires credentials at parse time (its CLI demands `--access-key`,
	/// defaulting from `AWS_ACCESS_KEY_ID`/`AWS_SECRET_ACCESS_KEY`/
	/// `AWS_SESSION_TOKEN`) — it does **not** honor the container-creds endpoint.
	/// So pass creds via those env vars (real for direct ops, dummy under the
	/// proxy), and **scrub** the IRSA / container-creds sources the pod injects so
	/// they can't shadow them in minio-go's chain.
	fn apply(&self, cmd: &mut Command) {
		cmd.env("KOPIA_PASSWORD", &self.password);
		cmd.env("AWS_ACCESS_KEY_ID", &self.access_key_id);
		cmd.env("AWS_SECRET_ACCESS_KEY", &self.secret_access_key);
		if self.session_token.is_empty() {
			cmd.env_remove("AWS_SESSION_TOKEN");
		} else {
			cmd.env("AWS_SESSION_TOKEN", &self.session_token);
		}
		for shadowing in [
			"AWS_WEB_IDENTITY_TOKEN_FILE",
			"AWS_ROLE_ARN",
			"AWS_CONTAINER_CREDENTIALS_FULL_URI",
			"AWS_CONTAINER_AUTHORIZATION_TOKEN",
			"AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
			"AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE",
		] {
			cmd.env_remove(shadowing);
		}
		if let Some(region) = &self.region {
			cmd.env("AWS_REGION", region);
			cmd.env("AWS_DEFAULT_REGION", region);
		}
	}

	/// The extra `s3` connector flags for the proxy, when one is in use:
	/// `--endpoint <host:port> --disable-tls`. Empty for the direct path.
	fn s3_endpoint_flags(&self) -> Vec<&str> {
		match &self.proxy_endpoint {
			Some(ep) => vec!["--endpoint", ep.as_str(), "--disable-tls"],
			None => Vec::new(),
		}
	}
}

// ===========================================================================
// Retention policy types + helpers (pure, tested).
// ===========================================================================

/// One per-type retention policy (one entry of the retention map). All fields
/// are optional/nullable: `null`/omitted means leave that dimension untouched
/// (kopia keeps its existing value).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Policy {
	#[serde(default)]
	pub keep_latest: Option<i64>,
	#[serde(default)]
	pub keep_daily: Option<i64>,
	#[serde(default)]
	pub keep_weekly: Option<i64>,
	#[serde(default)]
	pub keep_monthly: Option<i64>,
	#[serde(default)]
	pub keep_annual: Option<i64>,
}

impl Policy {
	/// Build `--keep-*` flag tokens for the present (Some) fields.
	fn flags(&self) -> Vec<String> {
		let mut f = Vec::new();
		let mut push = |name: &str, v: Option<i64>| {
			if let Some(n) = v {
				f.push(name.to_string());
				f.push(n.to_string());
			}
		};
		push("--keep-latest", self.keep_latest);
		push("--keep-daily", self.keep_daily);
		push("--keep-weekly", self.keep_weekly);
		push("--keep-monthly", self.keep_monthly);
		push("--keep-annual", self.keep_annual);
		f
	}
}

/// The per-type retention map (`{type → policy}`).
pub type RetentionMap = BTreeMap<String, Policy>;

/// Element-wise MAX (strictest) policy across every entry in the map; falls back
/// to the org floor when the map is empty. Used as a conservative global
/// baseline for `init` (no sources exist yet).
pub fn strictest_policy(map: &RetentionMap) -> Policy {
	if map.is_empty() {
		return Policy {
			keep_latest: Some(1),
			keep_daily: Some(7),
			keep_weekly: Some(4),
			keep_monthly: Some(6),
			keep_annual: None,
		};
	}
	let max_of = |f: fn(&Policy) -> Option<i64>| map.values().filter_map(f).max();
	Policy {
		keep_latest: max_of(|p| p.keep_latest),
		keep_daily: max_of(|p| p.keep_daily),
		keep_weekly: max_of(|p| p.keep_weekly),
		keep_monthly: max_of(|p| p.keep_monthly),
		keep_annual: max_of(|p| p.keep_annual),
	}
}

// ===========================================================================
// kopia snapshot-manifest parsing (pure, tested).
// ===========================================================================

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KSource {
	#[serde(rename = "userName")]
	user_name: String,
	host: String,
	path: String,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct KStats {
	#[serde(rename = "totalSize", default)]
	total_size: i64,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct Manifest {
	source: KSource,
	#[serde(rename = "startTime")]
	start_time: Option<String>,
	#[serde(default)]
	stats: KStats,
}

impl KSource {
	fn full(&self) -> String {
		format!("{}@{}:{}", self.user_name, self.host, self.path)
	}
}

/// One per-source entry in the inspect result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceEntry {
	/// Full kopia source `user@host:path`.
	pub source: String,
	/// The host part of the source (canopy server id), or `None`.
	pub server_id: Option<String>,
	/// Last path segment when it looks like a backup type, else `None`.
	pub type_: Option<String>,
	/// RFC3339 timestamp of the most recent snapshot for that source.
	pub latest_snapshot_at: Option<String>,
}

/// Extract the backup type from a source path: the part after the last `/`,
/// when it matches `^[a-z0-9-]+$` AND contains a non-digit; else `None`.
pub fn type_from_path(path: &str) -> Option<String> {
	let seg = path.rsplit('/').next().unwrap_or(path);
	if seg.is_empty() {
		return None;
	}
	let ok_charset = seg
		.chars()
		.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
	let has_non_digit = seg.chars().any(|c| !c.is_ascii_digit());
	if ok_charset && has_non_digit {
		Some(seg.to_string())
	} else {
		None
	}
}

/// Reduced view of a snapshot-manifest list, used by the inspect kind.
#[derive(Debug, PartialEq, Eq)]
pub struct Inspected {
	pub snapshot_count: i32,
	pub source_count: i32,
	pub logical_bytes: i64,
	pub sources: Vec<SourceEntry>,
}

/// Reduce a parsed snapshot-manifest list to per-source latest entries + counts
/// + summed logical bytes (sum over latest-per-source of `totalSize`).
pub fn inspect_manifests(manifests: &[Manifest]) -> Inspected {
	// Group by full source identity, keeping the latest by startTime.
	let mut groups: BTreeMap<String, &Manifest> = BTreeMap::new();
	for m in manifests {
		let key = m.source.full();
		match groups.get(&key) {
			Some(existing) if existing.start_time >= m.start_time => {}
			_ => {
				groups.insert(key, m);
			}
		}
	}

	let logical_bytes = groups.values().map(|m| m.stats.total_size).sum();

	let sources = groups
		.values()
		.map(|m| SourceEntry {
			source: m.source.full(),
			server_id: Some(m.source.host.clone()),
			type_: type_from_path(&m.source.path),
			latest_snapshot_at: m.start_time.clone(),
		})
		.collect();

	Inspected {
		snapshot_count: manifests.len() as i32,
		source_count: groups.len() as i32,
		logical_bytes,
		sources,
	}
}

/// Distinct sources (`user@host:path`) present in a manifest list.
pub fn distinct_sources(manifests: &[Manifest]) -> Vec<String> {
	let mut set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
	for m in manifests {
		set.insert(m.source.full());
	}
	set.into_iter().collect()
}

// ===========================================================================
// `kopia content stats` "Total Bytes:" parsing (pure, tested).
// ===========================================================================

/// Best-effort parse of `kopia content stats` text for a line like
/// `Total Bytes: 1.7 KB` into a byte count (1024-based; B/KB/MB/GB/TB). Returns
/// `None` if no such line / unit is found. Approximate — kopia rounds; the
/// authoritative physical size is the s3-metrics bucket_bytes figure.
pub fn parse_total_bytes(text: &str) -> Option<i64> {
	for line in text.lines() {
		let line = line.trim();
		let Some(rest) = line.strip_prefix("Total Bytes:") else {
			continue;
		};
		let rest = rest.trim();
		let mut parts = rest.split_whitespace();
		let value: f64 = parts.next()?.parse().ok()?;
		let unit = parts.next()?;
		let mult: f64 = match unit {
			"B" => 1.0,
			"KB" => 1024.0,
			"MB" => 1024.0 * 1024.0,
			"GB" => 1024.0 * 1024.0 * 1024.0,
			"TB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
			_ => return None,
		};
		return Some((value * mult) as i64);
	}
	None
}

// ===========================================================================
// kopia subprocess helpers.
// ===========================================================================

/// Run `kopia <args...>` to completion with the per-op env applied (assumed AWS
/// role + KOPIA_PASSWORD); the projected SA token file is inherited. Returns the
/// captured output.
pub async fn run_kopia(env: &KopiaEnv, args: &[&str]) -> Result<Output> {
	let mut cmd = Command::new("kopia");
	cmd.args(args);
	env.apply(&mut cmd);
	let out = cmd
		.output()
		.await
		.with_context(|| format!("failed to spawn kopia {}", args.join(" ")))?;
	Ok(out)
}

/// Like [`run_kopia`], but errors (with stderr context) when kopia exits
/// non-zero.
pub async fn run_kopia_ok(env: &KopiaEnv, args: &[&str]) -> Result<Output> {
	let out = run_kopia(env, args).await?;
	if !out.status.success() {
		bail!(
			"kopia {} failed: {}",
			args.join(" "),
			short_stderr(&out.stderr)
		);
	}
	Ok(out)
}

/// Run `kopia <args...> --json` and serde-parse stdout into `T`.
pub async fn kopia_json<T: serde::de::DeserializeOwned>(
	env: &KopiaEnv,
	args: &[&str],
) -> Result<T> {
	let out = run_kopia_ok(env, args).await?;
	serde_json::from_slice(&out.stdout)
		.with_context(|| format!("failed to parse JSON from kopia {}", args.join(" ")))
}

/// Trim + truncate stderr to a short single-line message for error context.
fn short_stderr(stderr: &[u8]) -> String {
	let s = String::from_utf8_lossy(stderr);
	let one_line = s.replace('\n', " ");
	let trimmed = one_line.trim();
	trimmed.chars().take(300).collect()
}

/// Connect to the repo with the fixed canopy maintenance identity so this client
/// IS the maintenance owner (required for `maintenance run`; harmless for
/// read-only inspect).
pub async fn connect(env: &KopiaEnv, bucket: &str, prefix: &str, region: &str) -> Result<()> {
	let mut args = vec![
		"repository",
		"connect",
		"s3",
		"--bucket",
		bucket,
		"--prefix",
		prefix,
		"--region",
		region,
		"--override-username",
		MAINTENANCE_USER,
		"--override-hostname",
		MAINTENANCE_HOST,
	];
	args.extend(env.s3_endpoint_flags());
	run_kopia_ok(env, &args)
		.await
		.context("cannot connect to repository")?;
	Ok(())
}

/// Rotate the repo passphrase to `new_password`. kopia `change-password` is an
/// O(1) metadata op (it re-wraps the `kopia.repository` format blob around the
/// unchanged master key — no content is re-encrypted), so this is cheap enough
/// to run frequently. `env.password` must be the *current* passphrase; the new
/// one is passed via `KOPIA_NEW_PASSWORD` (not argv, to keep it out of the
/// process list).
///
/// We verify by reconnecting with the new passphrase before returning Ok. The
/// two format-blob writes aren't atomic (kopia #3049): a failure between them
/// can leave the repo openable by *neither* passphrase, so the caller MUST keep
/// the old passphrase recorded and only publish the new one to the Secret once
/// this returns Ok.
pub async fn change_password(
	env: &KopiaEnv,
	bucket: &str,
	prefix: &str,
	region: &str,
	new_password: &str,
) -> Result<()> {
	connect(env, bucket, prefix, region)
		.await
		.context("change-password: connect with current passphrase failed")?;

	let mut cmd = Command::new("kopia");
	cmd.args(["repository", "change-password"]);
	env.apply(&mut cmd);
	cmd.env("KOPIA_NEW_PASSWORD", new_password);
	let out = cmd
		.output()
		.await
		.context("failed to spawn kopia repository change-password")?;
	if !out.status.success() {
		bail!(
			"kopia repository change-password failed: {}",
			short_stderr(&out.stderr)
		);
	}

	// Verify the rotation took: reconnect with the NEW passphrase.
	let verify_env = KopiaEnv {
		password: new_password.to_string(),
		..env.clone()
	};
	connect(&verify_env, bucket, prefix, region)
		.await
		.context("change-password: verify reconnect with the new passphrase failed")?;
	Ok(())
}

/// Apply a retention policy to a kopia policy target (`--global` or a source).
async fn apply_policy(env: &KopiaEnv, target: &str, policy: &Policy) -> Result<()> {
	let flags = policy.flags();
	if flags.is_empty() {
		return Ok(());
	}
	let mut args: Vec<&str> = vec!["policy", "set", target];
	for f in &flags {
		args.push(f);
	}
	run_kopia_ok(env, &args).await?;
	Ok(())
}

// ===========================================================================
// Per-kind orchestration.
// ===========================================================================

/// Initialise a group's repo and set the canopy maintenance identity + global
/// baseline policy.
///
/// `create_new` is true for from-birth (Canopy generates the passphrase for a
/// *new* repo): `repository create` runs, falling back to connect if the repo
/// already exists (idempotent retry). It is false for passphrase mode (connect
/// to an *existing* repo with the operator's passphrase): we **never** create —
/// Canopy must not mint a repo under an operator-chosen passphrase, so a missing
/// repo or wrong passphrase surfaces as an init failure.
pub async fn run_init(
	env: &KopiaEnv,
	bucket: &str,
	prefix: &str,
	region: &str,
	retention: &RetentionMap,
	create_new: bool,
) -> Result<()> {
	if create_new {
		let mut create_args = vec![
			"repository",
			"create",
			"s3",
			"--bucket",
			bucket,
			"--prefix",
			prefix,
			"--region",
			region,
		];
		create_args.extend(env.s3_endpoint_flags());
		let create = run_kopia(env, &create_args).await?;
		if !create.status.success() {
			connect(env, bucket, prefix, region)
				.await
				.context("repository create failed and connect fallback failed")?;
		}
	} else {
		// Passphrase mode: connect to the existing repo only — never create.
		connect(env, bucket, prefix, region).await.context(
			"connect to existing repository failed (passphrase mode never creates a repo)",
		)?;
	}

	// Connect with the fixed canopy identity (create leaves us connected, but be
	// explicit so we own maintenance regardless of path). Idempotent.
	connect(env, bucket, prefix, region).await?;

	// No sources exist yet: set a conservative GLOBAL baseline = the element-wise
	// strictest (max) policy across the map (or the org floor when empty).
	apply_policy(env, "--global", &strictest_policy(retention)).await?;

	// Make canopy the maintenance owner and disable automatic quick/full
	// maintenance so device clients never run delete-needing maintenance.
	run_kopia_ok(
		env,
		&[
			"maintenance",
			"set",
			"--owner",
			&format!("{MAINTENANCE_USER}@{MAINTENANCE_HOST}"),
		],
	)
	.await?;
	run_kopia_ok(
		env,
		&[
			"maintenance",
			"set",
			"--enable-quick",
			"false",
			"--enable-full",
			"false",
		],
	)
	.await?;
	Ok(())
}

/// Outcome of a maintenance run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintOutcome {
	/// Bytes freed. Always `None`: kopia maintenance emits no stable
	/// machine-readable bytes-reclaimed figure.
	pub bytes_reclaimed: Option<i64>,
}

/// Connect, assert per-source retention, expire+delete, then run maintenance
/// (`--full` for the full variant).
pub async fn run_maintenance(
	env: &KopiaEnv,
	bucket: &str,
	prefix: &str,
	region: &str,
	kind: MaintenanceKind,
	retention: &RetentionMap,
) -> Result<MaintOutcome> {
	connect(env, bucket, prefix, region).await?;

	// Assert per-source retention. List sources, and for each source whose type
	// (the path tail) is a key in the retention map, set that type's policy.
	let manifests: Vec<Manifest> = kopia_json(env, &["snapshot", "list", "--all", "--json"])
		.await
		.context("snapshot list failed")?;
	for source in distinct_sources(&manifests) {
		// type = the part after the last ':' (the path), then its path tail.
		let path = source.rsplit(':').next().unwrap_or(&source);
		let src_type = path.rsplit('/').next().unwrap_or(path);
		if let Some(policy) = retention.get(src_type) {
			apply_policy(env, &source, policy)
				.await
				.with_context(|| format!("setting policy for source {source}"))?;
		}
	}

	// Expire snapshots per policy and actually delete them (--delete is required;
	// without it expire is a dry run).
	run_kopia_ok(env, &["snapshot", "expire", "--all", "--delete"]).await?;

	if kind == MaintenanceKind::Full {
		run_kopia_ok(env, &["maintenance", "run", "--full"]).await?;
	} else {
		run_kopia_ok(env, &["maintenance", "run"]).await?;
	}

	Ok(MaintOutcome {
		bytes_reclaimed: None,
	})
}

/// Outcome of a read-only inspection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectOutcome {
	/// Result of `kopia snapshot verify` (`false` → corruption alert). A
	/// non-zero verify is NOT an error.
	pub verify_ok: bool,
	pub snapshot_count: i32,
	pub source_count: i32,
	pub logical_bytes: i64,
	/// Physical (stored) bytes from `kopia content stats`; `None` if unparseable.
	pub physical_bytes: Option<i64>,
	pub sources: Vec<SourceEntry>,
}

/// Connect, list snapshots, read physical stats, and verify (read-only). A
/// failing verify keeps `verify_ok:false` rather than erroring — the corruption
/// alert is raised off `verify_ok` by the completion logic.
pub async fn run_inspect(
	env: &KopiaEnv,
	bucket: &str,
	prefix: &str,
	region: &str,
) -> Result<InspectOutcome> {
	connect(env, bucket, prefix, region).await?;

	let manifests: Vec<Manifest> = kopia_json(env, &["snapshot", "list", "--all", "--json"])
		.await
		.context("snapshot list failed")?;
	let inspected = inspect_manifests(&manifests);

	// physical bytes: `kopia content stats` has no --json; parse the text line
	// "Total Bytes: <n> <unit>" best-effort. None if unavailable.
	let physical_bytes = match run_kopia(env, &["content", "stats"]).await {
		Ok(out) if out.status.success() => parse_total_bytes(&String::from_utf8_lossy(&out.stdout)),
		_ => None,
	};

	// verify: a non-zero verify does NOT fail the inspection; the completion
	// logic raises the corruption alert off verify_ok.
	let verify_ok = match run_kopia(env, &["snapshot", "verify"]).await {
		Ok(out) => out.status.success(),
		Err(_) => false,
	};

	Ok(InspectOutcome {
		verify_ok,
		snapshot_count: inspected.snapshot_count,
		source_count: inspected.source_count,
		logical_bytes: inspected.logical_bytes,
		physical_bytes,
		sources: inspected.sources,
	})
}

// ===========================================================================
// Tests (pure helpers — brought across from the old kopia-job crate).
// ===========================================================================

#[cfg(test)]
mod tests {
	use super::*;

	fn manifest(user: &str, host: &str, path: &str, start: &str, size: i64) -> Manifest {
		Manifest {
			source: KSource {
				user_name: user.to_string(),
				host: host.to_string(),
				path: path.to_string(),
			},
			start_time: Some(start.to_string()),
			stats: KStats { total_size: size },
		}
	}

	#[test]
	fn parse_snapshot_list_real_shape() {
		// A realistic kopia 0.23.1 `snapshot list --all --json` element.
		let json = r#"[
			{
				"source": {"host": "srv-1", "userName": "canopy", "path": "tamanu-postgres"},
				"startTime": "2026-06-18T12:00:00Z",
				"stats": {"totalSize": 1000}
			},
			{
				"source": {"host": "srv-1", "userName": "canopy", "path": "tamanu-postgres"},
				"startTime": "2026-06-18T13:00:00Z",
				"stats": {"totalSize": 1500}
			},
			{
				"source": {"host": "srv-2", "userName": "canopy", "path": "files/data"},
				"startTime": "2026-06-18T09:00:00Z",
				"stats": {"totalSize": 200}
			}
		]"#;
		let manifests: Vec<Manifest> = serde_json::from_str(json).unwrap();
		let got = inspect_manifests(&manifests);

		assert_eq!(got.snapshot_count, 3);
		assert_eq!(got.source_count, 2);
		// latest-per-source: srv-1 -> 1500, srv-2 -> 200
		assert_eq!(got.logical_bytes, 1700);

		let srv1 = got
			.sources
			.iter()
			.find(|s| s.source == "canopy@srv-1:tamanu-postgres")
			.unwrap();
		assert_eq!(srv1.server_id.as_deref(), Some("srv-1"));
		assert_eq!(srv1.type_.as_deref(), Some("tamanu-postgres"));
		assert_eq!(
			srv1.latest_snapshot_at.as_deref(),
			Some("2026-06-18T13:00:00Z")
		);

		let srv2 = got
			.sources
			.iter()
			.find(|s| s.source == "canopy@srv-2:files/data")
			.unwrap();
		// "data" is a valid type segment (the path tail).
		assert_eq!(srv2.type_.as_deref(), Some("data"));
	}

	#[test]
	fn inspect_empty_list() {
		let got = inspect_manifests(&[]);
		assert_eq!(got.snapshot_count, 0);
		assert_eq!(got.source_count, 0);
		assert_eq!(got.logical_bytes, 0);
		assert!(got.sources.is_empty());
	}

	#[test]
	fn distinct_sources_dedups() {
		let manifests = vec![
			manifest("canopy", "a", "t1", "2026-01-01T00:00:00Z", 0),
			manifest("canopy", "a", "t1", "2026-01-02T00:00:00Z", 0),
			manifest("canopy", "b", "t2", "2026-01-01T00:00:00Z", 0),
		];
		assert_eq!(
			distinct_sources(&manifests),
			vec!["canopy@a:t1".to_string(), "canopy@b:t2".to_string()]
		);
	}

	#[test]
	fn type_extraction() {
		assert_eq!(
			type_from_path("tamanu-postgres").as_deref(),
			Some("tamanu-postgres")
		);
		assert_eq!(type_from_path("files/data").as_deref(), Some("data"));
		assert_eq!(type_from_path("a/b/my-type").as_deref(), Some("my-type"));
		// All-digit tail -> not a type.
		assert_eq!(type_from_path("12345"), None);
		assert_eq!(type_from_path("path/2026"), None);
		// Uppercase / invalid charset -> not a type.
		assert_eq!(type_from_path("Type"), None);
		assert_eq!(type_from_path("under_score"), None);
		assert_eq!(type_from_path("with space"), None);
		assert_eq!(type_from_path(""), None);
		// digit-with-letter is fine.
		assert_eq!(type_from_path("v2-db").as_deref(), Some("v2-db"));
	}

	#[test]
	fn strictest_policy_elementwise_max() {
		let mut map = RetentionMap::new();
		map.insert(
			"a".into(),
			Policy {
				keep_latest: Some(1),
				keep_daily: Some(7),
				keep_weekly: Some(4),
				keep_monthly: Some(6),
				keep_annual: None,
			},
		);
		map.insert(
			"b".into(),
			Policy {
				keep_latest: Some(3),
				keep_daily: Some(5),
				keep_weekly: Some(8),
				keep_monthly: None,
				keep_annual: Some(2),
			},
		);
		let p = strictest_policy(&map);
		assert_eq!(p.keep_latest, Some(3));
		assert_eq!(p.keep_daily, Some(7));
		assert_eq!(p.keep_weekly, Some(8));
		assert_eq!(p.keep_monthly, Some(6));
		assert_eq!(p.keep_annual, Some(2));
	}

	#[test]
	fn strictest_policy_floor_when_empty() {
		let p = strictest_policy(&RetentionMap::new());
		assert_eq!(p.keep_daily, Some(7));
		assert_eq!(p.keep_weekly, Some(4));
		assert_eq!(p.keep_monthly, Some(6));
		assert_eq!(p.keep_latest, Some(1));
		assert_eq!(p.keep_annual, None);
	}

	#[test]
	fn policy_flags_skips_none() {
		let p = Policy {
			keep_latest: Some(1),
			keep_daily: None,
			keep_weekly: Some(4),
			keep_monthly: None,
			keep_annual: None,
		};
		assert_eq!(p.flags(), vec!["--keep-latest", "1", "--keep-weekly", "4"]);
		assert!(Policy::default().flags().is_empty());
	}

	#[test]
	fn retention_map_deserializes() {
		let raw = r#"{
			"tamanu-postgres": {"keep_daily": 7, "keep_weekly": 4, "keep_monthly": 6, "keep_latest": null}
		}"#;
		let map: RetentionMap = serde_json::from_str(raw).unwrap();
		let p = map.get("tamanu-postgres").unwrap();
		assert_eq!(p.keep_daily, Some(7));
		assert_eq!(p.keep_latest, None);
	}

	#[test]
	fn parse_total_bytes_units() {
		assert_eq!(parse_total_bytes("Total Bytes: 0 B"), Some(0));
		assert_eq!(parse_total_bytes("Total Bytes: 1.7 KB"), Some(1740)); // 1.7*1024 = 1740.8
		assert_eq!(
			parse_total_bytes("Total Bytes: 2 MB"),
			Some(2 * 1024 * 1024)
		);
		assert_eq!(
			parse_total_bytes("Total Bytes: 1 GB"),
			Some(1024 * 1024 * 1024)
		);
		assert_eq!(
			parse_total_bytes("Total Bytes: 1 TB"),
			Some(1024i64 * 1024 * 1024 * 1024)
		);
	}

	#[test]
	fn parse_total_bytes_in_multiline() {
		let text = "Count: 3\nTotal Bytes: 1.7 KB\nOther: x\n";
		assert_eq!(parse_total_bytes(text), Some(1740));
	}

	#[test]
	fn parse_total_bytes_missing_or_unknown_unit() {
		assert_eq!(parse_total_bytes("Count: 3\n"), None);
		assert_eq!(parse_total_bytes("Total Bytes: 5 PB"), None);
		assert_eq!(parse_total_bytes("Total Bytes:"), None);
	}
}
