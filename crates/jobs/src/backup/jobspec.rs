//! Build the per-group kopia k8s Job manifest. The scheduler fills these in
//! and hands them to a [`super::spawn::JobSpawner`]. The image entrypoint
//! contract (args, source-host convention, result reporting) is owned by ops
//! (spec §5); canopy only passes args + mounts the repo password via
//! `secretKeyRef` (never a plain-value env, never logged).

use std::collections::BTreeMap;

use commons_servers::backup_jobs::{BillingLabels, JobKind};
use k8s_openapi::api::{
	batch::v1::{Job, JobSpec},
	core::v1::{Container, EnvVar, EnvVarSource, PodSpec, PodTemplateSpec, SecretKeySelector},
};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use uuid::Uuid;

/// Everything needed to render one backup Job.
pub struct JobParams {
	pub namespace: String,
	pub kind: JobKind,
	pub group_id: Uuid,
	pub image: String,
	/// IRSA service account: maintenance/init → full-access SA; inspect →
	/// read-only SA. The caller picks per kind.
	pub service_account: String,
	pub bucket: String,
	pub prefix: String,
	pub region: Option<String>,
	pub target_role_arn: String,
	/// Floor-enforced retention JSON, passed to the assert-retention step.
	pub retention_json: String,
	/// k8s Secret name holding the repo password (`repo_password_ref`).
	pub repo_password_secret: String,
	/// Key within that Secret.
	pub repo_password_key: String,
	pub billing: BillingLabels,
	/// `backup_maintenance_runs.id` for maintenance kinds, so the Job can
	/// report completion against the right row. `None` for inspect/init.
	pub run_id: Option<i64>,
}

const PASSWORD_ENV: &str = "KOPIA_PASSWORD";
const TTL_AFTER_FINISHED_SECS: i32 = 3600;
const BACKOFF_LIMIT: i32 = 2;

/// The selection/billing labels carried on both the Job and its pod template.
pub fn labels(group_id: Uuid, kind: JobKind, billing: &BillingLabels) -> BTreeMap<String, String> {
	let mut m = BTreeMap::new();
	m.insert("canopy-group".to_string(), group_id.to_string());
	m.insert("canopy-backup-kind".to_string(), kind.as_str().to_string());
	m.insert("billing.product".to_string(), billing.product.clone());
	m.insert("billing.deployment".to_string(), billing.deployment.clone());
	if let Some(stage) = &billing.stage {
		m.insert("billing.stage".to_string(), stage.clone());
	}
	m
}

/// `metadata.name`-safe label value for a Kubernetes label (≤63 chars; here
/// always small ints / kinds / uuids, so just stringify).
fn run_id_label(run_id: i64) -> String {
	run_id.to_string()
}

/// The kopia entrypoint args for this kind (non-secret config; the password is
/// an env from `secretKeyRef`, never an arg).
fn args(p: &JobParams) -> Vec<String> {
	let mut a = vec![
		p.kind.as_str().to_string(),
		"--bucket".to_string(),
		p.bucket.clone(),
		"--prefix".to_string(),
		p.prefix.clone(),
		"--role-arn".to_string(),
		p.target_role_arn.clone(),
	];
	if let Some(region) = &p.region {
		a.push("--region".to_string());
		a.push(region.clone());
	}
	match p.kind {
		JobKind::MaintQuick | JobKind::MaintFull | JobKind::Init => {
			a.push("--retention".to_string());
			a.push(p.retention_json.clone());
		}
		JobKind::Inspect => {}
	}
	if let Some(run_id) = p.run_id {
		a.push("--run-id".to_string());
		a.push(run_id.to_string());
	}
	a
}

/// Render the `batch/v1` Job. `generateName` keeps each spawn uniquely named;
/// `ttlSecondsAfterFinished` self-prunes finished Jobs; `restartPolicy: Never`
/// matches the migrator/chrome-versions Jobs.
pub fn build_job(p: &JobParams) -> Job {
	let mut labels = labels(p.group_id, p.kind, &p.billing);
	// Maintenance Jobs carry their run id so completion polling can map a
	// finished Job back to the backup_maintenance_runs row to close.
	if let Some(run_id) = p.run_id {
		labels.insert("canopy-run-id".to_string(), run_id_label(run_id));
	}
	let group_short = p.group_id.simple().to_string()[..8].to_string();

	let container = Container {
		name: "kopia".to_string(),
		image: Some(p.image.clone()),
		args: Some(args(p)),
		env: Some(vec![EnvVar {
			name: PASSWORD_ENV.to_string(),
			value_from: Some(EnvVarSource {
				secret_key_ref: Some(SecretKeySelector {
					name: p.repo_password_secret.clone(),
					key: p.repo_password_key.clone(),
					optional: Some(false),
				}),
				..Default::default()
			}),
			..Default::default()
		}]),
		..Default::default()
	};

	let pod_spec = PodSpec {
		containers: vec![container],
		restart_policy: Some("Never".to_string()),
		service_account_name: Some(p.service_account.clone()),
		..Default::default()
	};

	Job {
		metadata: ObjectMeta {
			generate_name: Some(format!("canopy-backup-{}-{group_short}-", p.kind.as_str())),
			namespace: Some(p.namespace.clone()),
			labels: Some(labels.clone()),
			..Default::default()
		},
		spec: Some(JobSpec {
			backoff_limit: Some(BACKOFF_LIMIT),
			ttl_seconds_after_finished: Some(TTL_AFTER_FINISHED_SECS),
			template: PodTemplateSpec {
				metadata: Some(ObjectMeta {
					labels: Some(labels),
					..Default::default()
				}),
				spec: Some(pod_spec),
			},
			..Default::default()
		}),
		..Default::default()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn params(kind: JobKind) -> JobParams {
		JobParams {
			namespace: "tamanu-meta-test".to_string(),
			kind,
			group_id: Uuid::from_u128(0x1234),
			image: "ghcr.io/bes/kopia-job:latest".to_string(),
			service_account: "canopy-maintenance".to_string(),
			bucket: "bes-kopia-backups-test".to_string(),
			prefix: String::new(),
			region: Some("ap-southeast-2".to_string()),
			target_role_arn: "arn:aws:iam::123456789012:role/canopy-backups-test".to_string(),
			retention_json: r#"{"keep_daily":7}"#.to_string(),
			repo_password_secret: "kopia-repo-pw-test".to_string(),
			repo_password_key: "password".to_string(),
			billing: BillingLabels {
				product: "tamanu".to_string(),
				deployment: "test-deploy".to_string(),
				stage: Some("prod".to_string()),
			},
			run_id: Some(42),
		}
	}

	#[test]
	fn password_is_secret_key_ref_never_a_plain_value_or_arg() {
		let job = build_job(&params(JobKind::MaintFull));
		let env = &job
			.spec
			.as_ref()
			.unwrap()
			.template
			.spec
			.as_ref()
			.unwrap()
			.containers[0]
			.env
			.as_ref()
			.unwrap()[0];
		assert_eq!(env.name, PASSWORD_ENV);
		assert!(
			env.value.is_none(),
			"password must NOT be a plain-value env"
		);
		assert_eq!(
			env.value_from
				.as_ref()
				.unwrap()
				.secret_key_ref
				.as_ref()
				.unwrap()
				.name,
			"kopia-repo-pw-test"
		);
		// And never in the args.
		let args = job
			.spec
			.as_ref()
			.unwrap()
			.template
			.spec
			.as_ref()
			.unwrap()
			.containers[0]
			.args
			.as_ref()
			.unwrap();
		assert!(
			!args
				.iter()
				.any(|a| a.contains("kopia-repo-pw") || a == "password")
		);
	}

	#[test]
	fn carries_selection_and_billing_labels_and_safety_fields() {
		let job = build_job(&params(JobKind::Inspect));
		let labels = job.metadata.labels.as_ref().unwrap();
		assert_eq!(labels.get("canopy-backup-kind").unwrap(), "inspect");
		assert_eq!(labels.get("billing.stage").unwrap(), "prod");
		assert!(labels.contains_key("canopy-group"));
		let spec = job.spec.as_ref().unwrap();
		assert_eq!(spec.backoff_limit, Some(BACKOFF_LIMIT));
		assert_eq!(
			spec.ttl_seconds_after_finished,
			Some(TTL_AFTER_FINISHED_SECS)
		);
		let pod = spec.template.spec.as_ref().unwrap();
		assert_eq!(pod.restart_policy.as_deref(), Some("Never"));
		assert_eq!(
			pod.service_account_name.as_deref(),
			Some("canopy-maintenance")
		);
	}

	#[test]
	fn inspect_omits_retention_arg_maint_includes_it() {
		let inspect = build_job(&params(JobKind::Inspect));
		let iargs = inspect
			.spec
			.unwrap()
			.template
			.spec
			.unwrap()
			.containers
			.remove(0)
			.args
			.unwrap();
		assert!(!iargs.iter().any(|a| a == "--retention"));
		let maint = build_job(&params(JobKind::MaintQuick));
		let margs = maint
			.spec
			.unwrap()
			.template
			.spec
			.unwrap()
			.containers
			.remove(0)
			.args
			.unwrap();
		assert!(margs.iter().any(|a| a == "--retention"));
	}

	#[test]
	fn stage_omitted_when_none() {
		let mut p = params(JobKind::MaintQuick);
		p.billing.stage = None;
		let job = build_job(&p);
		assert!(!job.metadata.labels.unwrap().contains_key("billing.stage"));
	}
}
