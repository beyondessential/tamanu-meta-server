-- The repo inventory recorded only the newest snapshot time per source, so
-- canopy held the snapshot id a device reported (backup_runs.snapshot_id) with
-- nothing to match it against: reconciliation could compare timestamps and no
-- more. This records the identity of every snapshot an inspection observes, so
-- "the device reported a snapshot the repository does not hold" is a question
-- canopy can answer rather than infer.
--
-- One row per snapshot per group, keyed by the snapshot's own id, which is
-- unique within a repository. The set is the last inspection's observation, not
-- a history: each inspection replaces it, so a snapshot that retention has
-- since expired stops being recorded and the table stays the size of the repo.
CREATE TABLE backup_repo_observed_snapshots (
	group_id UUID NOT NULL REFERENCES server_groups (id) ON DELETE CASCADE,
	snapshot_id TEXT NOT NULL,
	source TEXT NOT NULL,
	snapshot_at TIMESTAMPTZ,
	observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (group_id, snapshot_id)
);
