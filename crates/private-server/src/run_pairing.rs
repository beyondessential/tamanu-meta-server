//! Pairing device-reported runs/checks with the credential issuances that
//! started them. A credential issuance marks a run's *start*; the report marks
//! its *end*. Pairing recovers the start time (and thus the Canopy-measured
//! duration) and turns issuances that never reported into inferred activity
//! rows. Shared by the backup-runs ([`crate::fns::backups`]) and restore-checks
//! ([`crate::fns::restore_replicas`]) views.

use std::collections::HashMap;

use commons_types::backup::{BackupPurpose, BackupType};
use database::backups::BackupCredentialIssuance;
use jiff::Timestamp;
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

/// State of an activity row: a device-reported run, or a run inferred from a
/// credential issuance that never matched a report.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
	/// The device reported this run's outcome; the outcome fields are populated.
	Reported,
	/// Inferred from a credential issuance whose credentials are still valid and
	/// which has no matching report yet — a run believed to be in flight.
	InProgress,
	/// Inferred from a credential issuance whose credentials have expired with no
	/// matching report. The run happened but its outcome was never reported (the
	/// current state of manual `bestool canopy restore`, which doesn't report).
	Unknown,
}

/// Grace added to a credential's `expires_at` when deciding whether an issuance
/// belongs to a run's issuance chain — absorbs report lag and clock skew.
pub const CRED_GRACE_SECS: i64 = 15 * 60;

/// How far back to fetch issuances for an activity view. Bounds the merge when a
/// group has reports but sparse issuances (or only issuances and no reports).
pub const ISSUANCE_LOOKBACK_SECS: i64 = 3 * 24 * 3600;

/// Extra lookback before the oldest displayed report, so a long run's first
/// issuance (minted before the report) is still fetched.
pub const CHAIN_LOOKBACK_SECS: i64 = 2 * 3600;

/// The `since` bound for fetching issuances to merge with the displayed reports.
/// Looks back at least [`ISSUANCE_LOOKBACK_SECS`], and further when the oldest
/// displayed report predates that (to cover its issuance chain).
pub fn issuance_since(now: Timestamp, oldest_report: Option<Timestamp>) -> Timestamp {
	let base = now.as_second() - ISSUANCE_LOOKBACK_SECS;
	let secs = match oldest_report {
		Some(t) => base.min(t.as_second() - CHAIN_LOOKBACK_SECS),
		None => base,
	};
	Timestamp::from_second(secs).unwrap_or(now)
}

/// Key grouping issuances/reports that could belong to the same run when no
/// exact `run_id` correlation is available.
pub type RunKey = (Uuid, BackupType, BackupPurpose);

pub fn run_key(device_id: Uuid, r#type: &BackupType, purpose: BackupPurpose) -> RunKey {
	(device_id, r#type.clone(), purpose)
}

/// A report to be paired against issuances: its optional correlation id (`None`
/// for legacy clients), its grouping key for the fallback, and its end time.
pub struct ReportRef {
	pub run_id: Option<Uuid>,
	pub key: RunKey,
	pub reported_at: Timestamp,
}

/// An issuance chain with no matching report — an inferred (unreported) run.
pub struct Attempt {
	/// The earliest issuance of the chain (the run's start).
	pub first: BackupCredentialIssuance,
	/// The latest credential expiry across the chain.
	pub latest_expires: Timestamp,
}

impl Attempt {
	/// In flight while the creds are still valid; otherwise a past run whose
	/// outcome was never reported.
	pub fn status(&self, now: Timestamp) -> RunStatus {
		if now < self.latest_expires {
			RunStatus::InProgress
		} else {
			RunStatus::Unknown
		}
	}
}

/// Pair reports with the issuances that started them. Returns, aligned to
/// `reports`, each report's start time (the earliest issuance of its chain, or
/// `None` when nothing matched), plus the leftover issuance chains as inferred
/// [`Attempt`]s.
///
/// Pairing is exact when a report and its issuances share a `run_id`. Issuances
/// without a `run_id` (older clients) fall back to the time-window contiguity
/// guesstimate in [`claim_chain_for_report`] — remove that fallback once
/// `run_id` is mandatory on the credential call. `reports` must be newest-first
/// so a later run claims the later chain in the guesstimate path.
pub fn pair_issuances(
	issuances: Vec<BackupCredentialIssuance>,
	reports: &[ReportRef],
) -> (Vec<Option<Timestamp>>, Vec<Attempt>) {
	// Correlated issuances group exactly by the run they were minted for;
	// uncorrelated ones go through the guesstimate, per key, ascending.
	let mut by_run_id: HashMap<Uuid, Vec<BackupCredentialIssuance>> = HashMap::new();
	let mut by_key: HashMap<RunKey, Vec<(BackupCredentialIssuance, bool)>> = HashMap::new();
	for iss in issuances {
		match iss.run_id {
			Some(rid) => by_run_id.entry(rid).or_default().push(iss),
			None => by_key
				.entry(run_key(iss.device_id, &iss.r#type, iss.purpose))
				.or_default()
				.push((iss, false)),
		}
	}
	for chain in by_key.values_mut() {
		chain.sort_by_key(|(i, _)| i.issued_at);
	}

	let mut starts = Vec::with_capacity(reports.len());
	for report in reports {
		let started = report
			.run_id
			.and_then(|rid| by_run_id.remove(&rid))
			.and_then(|chain| chain.iter().map(|i| i.issued_at).min())
			.or_else(|| {
				by_key
					.get_mut(&report.key)
					.and_then(|chain| claim_chain_for_report(chain, report.reported_at))
			});
		starts.push(started);
	}

	let mut attempts = Vec::new();
	// Correlated issuances with no matching report → one attempt per run_id.
	for chain in by_run_id.into_values() {
		let first = chain
			.iter()
			.min_by_key(|i| i.issued_at)
			.cloned()
			.expect("non-empty chain");
		let latest_expires = chain.iter().map(|i| i.expires_at).max().unwrap();
		attempts.push(Attempt {
			first,
			latest_expires,
		});
	}
	// Uncorrelated leftovers → one attempt per contiguous chain (a run re-mints
	// creds roughly hourly, so its issuances overlap within the credential
	// lifetime); not one per re-mint. (Guesstimate fallback.)
	for chain in by_key.into_values() {
		let mut idx = 0;
		while idx < chain.len() {
			if chain[idx].1 {
				idx += 1;
				continue;
			}
			let start = idx;
			let mut end = idx;
			let mut latest_expires = chain[idx].0.expires_at;
			while end + 1 < chain.len()
				&& !chain[end + 1].1
				&& chain[end + 1].0.issued_at.as_second()
					<= latest_expires.as_second() + CRED_GRACE_SECS
			{
				end += 1;
				latest_expires = latest_expires.max(chain[end].0.expires_at);
			}
			attempts.push(Attempt {
				first: chain[start].0.clone(),
				latest_expires,
			});
			idx = end + 1;
		}
	}

	(starts, attempts)
}

/// Claim the issuance chain for a reported run and return its start time (the
/// earliest issuance in the chain). Walks back from the latest issuance at or
/// before `reported_at` while consecutive issuances stay contiguous (a re-mint
/// overlaps the prior creds' validity, within grace), stopping at the first gap.
/// Returns `None` — claiming nothing — when the latest candidate's creds had
/// already expired well before the report (so it belongs to no live chain).
fn claim_chain_for_report(
	chain: &mut [(BackupCredentialIssuance, bool)],
	reported_at: Timestamp,
) -> Option<Timestamp> {
	// Latest unclaimed issuance with issued_at <= reported_at.
	let top = chain
		.iter()
		.rposition(|(i, claimed)| !claimed && i.issued_at <= reported_at)?;
	// The report must fall within the top issuance's validity (plus grace);
	// otherwise this issuance isn't the tail of this run's chain.
	if reported_at.as_second() > chain[top].0.expires_at.as_second() + CRED_GRACE_SECS {
		return None;
	}
	let mut start = top;
	while start > 0 {
		let prev = start - 1;
		if chain[prev].1 {
			break;
		}
		// prev is contiguous if its creds' validity (plus grace) reaches the next
		// issuance's start — i.e. it's a re-mint of the same ongoing run.
		if chain[prev].0.expires_at.as_second() + CRED_GRACE_SECS
			>= chain[start].0.issued_at.as_second()
		{
			start = prev;
		} else {
			break;
		}
	}
	let started_at = chain[start].0.issued_at;
	for entry in &mut chain[start..=top] {
		entry.1 = true;
	}
	Some(started_at)
}
