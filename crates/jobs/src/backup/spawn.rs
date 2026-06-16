//! [`JobSpawner`] — the thin abstraction over the kube API the schedulers use
//! to create backup Jobs and to check what's already running (avoiding
//! double-spawn). Kept behind a trait so the scheduler decision logic is
//! testable with [`FakeSpawner`] without standing up a cluster.

use std::{collections::HashSet, future::Future};

use k8s_openapi::api::batch::v1::Job;
use kube::api::{ListParams, PostParams};
use uuid::Uuid;

/// String-keyed error: spawning is infra orchestration, so callers log + carry
/// on rather than match on variants.
pub type Result<T> = std::result::Result<T, String>;

/// A finished backup Job observed by the completion poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishedJob {
	pub name: String,
	pub group_id: Option<Uuid>,
	/// `backup_maintenance_runs.id` from the `canopy-run-id` label (maintenance
	/// Jobs only).
	pub run_id: Option<i64>,
	pub kind: Option<String>,
	/// `true` if the Job reported `succeeded`, `false` if `failed`.
	pub succeeded: bool,
}

/// Create backup Jobs and observe which groups already have one running.
///
/// Methods are `Send`-bounded RPITIT (not bare `async fn`) so the futures can
/// be awaited inside the schedulers' `task::spawn`ed loops.
pub trait JobSpawner {
	/// Create the Job; returns the server-assigned name.
	fn spawn(&self, job: Job) -> impl Future<Output = Result<String>> + Send;

	/// Group ids that currently have an **active** backup Job of any kind (via
	/// the `canopy-group` label) — used to skip a group that's already mid-run.
	fn active_groups(&self) -> impl Future<Output = Result<HashSet<Uuid>>> + Send;

	/// Backup Jobs that have finished (succeeded or failed) — the completion
	/// poll reads these each tick to close `backup_maintenance_runs` rows, then
	/// [`delete_job`](Self::delete_job)s them.
	fn finished_jobs(&self) -> impl Future<Output = Result<Vec<FinishedJob>>> + Send;

	/// Delete a finished Job by name (after recording its outcome).
	fn delete_job(&self, name: &str) -> impl Future<Output = Result<()>> + Send;
}

/// The real spawner, backed by `kube::Api<Job>` in canopy's namespace.
pub struct KubeSpawner {
	api: kube::Api<Job>,
}

impl KubeSpawner {
	pub fn new(client: kube::Client, namespace: &str) -> Self {
		Self {
			api: kube::Api::namespaced(client, namespace),
		}
	}
}

impl JobSpawner for KubeSpawner {
	async fn spawn(&self, job: Job) -> Result<String> {
		let created = self
			.api
			.create(&PostParams::default(), &job)
			.await
			.map_err(|e| e.to_string())?;
		Ok(created.metadata.name.unwrap_or_default())
	}

	async fn active_groups(&self) -> Result<HashSet<Uuid>> {
		// Any backup Job carries `canopy-backup-kind`; select on it.
		let lp = ListParams::default().labels("canopy-backup-kind");
		let jobs = self.api.list(&lp).await.map_err(|e| e.to_string())?;
		let mut set = HashSet::new();
		for j in jobs {
			let active = j.status.as_ref().and_then(|s| s.active).unwrap_or(0);
			if active > 0
				&& let Some(g) = j
					.metadata
					.labels
					.as_ref()
					.and_then(|l| l.get("canopy-group"))
				&& let Ok(u) = Uuid::parse_str(g)
			{
				set.insert(u);
			}
		}
		Ok(set)
	}

	async fn finished_jobs(&self) -> Result<Vec<FinishedJob>> {
		let lp = ListParams::default().labels("canopy-backup-kind");
		let jobs = self.api.list(&lp).await.map_err(|e| e.to_string())?;
		let mut out = Vec::new();
		for j in jobs {
			let status = j.status.as_ref();
			let succeeded = status.and_then(|s| s.succeeded).unwrap_or(0) > 0;
			let failed = status.and_then(|s| s.failed).unwrap_or(0) > 0;
			if !succeeded && !failed {
				continue;
			}
			let labels = j.metadata.labels.as_ref();
			out.push(FinishedJob {
				name: j.metadata.name.clone().unwrap_or_default(),
				group_id: labels
					.and_then(|l| l.get("canopy-group"))
					.and_then(|g| Uuid::parse_str(g).ok()),
				run_id: labels
					.and_then(|l| l.get("canopy-run-id"))
					.and_then(|r| r.parse::<i64>().ok()),
				kind: labels.and_then(|l| l.get("canopy-backup-kind").cloned()),
				succeeded,
			});
		}
		Ok(out)
	}

	async fn delete_job(&self, name: &str) -> Result<()> {
		use kube::api::DeleteParams;
		self.api
			.delete(name, &DeleteParams::background())
			.await
			.map_err(|e| e.to_string())?;
		Ok(())
	}
}

/// In-memory spawner for tests: records what it was asked to create and reports
/// a configurable already-active set.
pub struct FakeSpawner {
	pub spawned: std::sync::Mutex<Vec<Job>>,
	pub active: HashSet<Uuid>,
	pub finished: Vec<FinishedJob>,
	pub deleted: std::sync::Mutex<Vec<String>>,
}

impl FakeSpawner {
	pub fn new(active: HashSet<Uuid>) -> Self {
		Self {
			spawned: std::sync::Mutex::new(Vec::new()),
			active,
			finished: Vec::new(),
			deleted: std::sync::Mutex::new(Vec::new()),
		}
	}

	pub fn with_finished(mut self, finished: Vec<FinishedJob>) -> Self {
		self.finished = finished;
		self
	}

	pub fn spawned_kinds(&self) -> Vec<String> {
		self.spawned
			.lock()
			.unwrap()
			.iter()
			.filter_map(|j| {
				j.metadata
					.labels
					.as_ref()
					.and_then(|l| l.get("canopy-backup-kind").cloned())
			})
			.collect()
	}
}

impl JobSpawner for FakeSpawner {
	async fn spawn(&self, job: Job) -> Result<String> {
		let name = job.metadata.generate_name.clone().unwrap_or_default();
		self.spawned.lock().unwrap().push(job);
		Ok(name)
	}

	async fn active_groups(&self) -> Result<HashSet<Uuid>> {
		Ok(self.active.clone())
	}

	async fn finished_jobs(&self) -> Result<Vec<FinishedJob>> {
		Ok(self.finished.clone())
	}

	async fn delete_job(&self, name: &str) -> Result<()> {
		self.deleted.lock().unwrap().push(name.to_string());
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::backup::jobspec::{JobParams, build_job};
	use commons_servers::backup_jobs::{BillingLabels, JobKind};

	fn job(group: Uuid) -> Job {
		build_job(&JobParams {
			namespace: "ns".into(),
			kind: JobKind::MaintQuick,
			group_id: group,
			image: "img".into(),
			service_account: "sa".into(),
			bucket: "b".into(),
			prefix: String::new(),
			region: None,
			target_role_arn: "arn".into(),
			retention_json: "{}".into(),
			repo_password_secret: "s".into(),
			repo_password_key: "k".into(),
			billing: BillingLabels {
				product: "tamanu".into(),
				deployment: "d".into(),
				stage: None,
			},
			run_id: Some(1),
		})
	}

	#[tokio::test(flavor = "multi_thread")]
	async fn fake_records_spawns_and_reports_active() {
		let g = Uuid::from_u128(5);
		let fake = FakeSpawner::new(HashSet::from([Uuid::from_u128(9)]));
		assert!(
			fake.active_groups()
				.await
				.unwrap()
				.contains(&Uuid::from_u128(9))
		);
		fake.spawn(job(g)).await.unwrap();
		assert_eq!(fake.spawned_kinds(), vec!["maint-quick".to_string()]);
	}
}
