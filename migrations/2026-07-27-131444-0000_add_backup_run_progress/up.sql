-- In-flight progress samples devices report while a backup or restore run is
-- under way. A run's own row in backup_runs doesn't exist until the run
-- *finishes*, so run_id carries no FK and the sample is self-describing on
-- device/group/server/type/purpose — same reasoning as
-- backup_credential_issuances, which likewise records activity before there's a
-- run to hang it off.
--
-- Every counter is cumulative from the start of the run, never per-interval, so
-- a dropped or repeated sample costs resolution but never corrupts a total. All
-- are nullable: a device omits whatever it doesn't measure.
--
-- observed_at is server-stamped rather than device-supplied. Rate is derived
-- from it, and "is this run moving, as far as Canopy can tell" is a receipt-time
-- question. snapshot_taken_at is the one unavoidable device clock claim: it
-- describes a freeze that happened on the device's own filesystem.
CREATE TABLE backup_run_progress (
	id                        BIGSERIAL PRIMARY KEY,
	run_id                    UUID NOT NULL,
	device_id                 UUID NOT NULL REFERENCES devices(id),
	group_id                  UUID NOT NULL REFERENCES server_groups(id),
	server_id                 UUID REFERENCES servers(id),
	type                      TEXT NOT NULL,
	purpose                   TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
	observed_at               TIMESTAMPTZ NOT NULL DEFAULT now(),
	snapshot_taken_at         TIMESTAMPTZ,

	-- Backup-engine counters, mapped by the client onto Canopy's own names.
	bytes_read                BIGINT,
	bytes_hashed              BIGINT,
	bytes_uploaded            BIGINT,
	bytes_cached              BIGINT,
	bytes_estimated           BIGINT,
	files_done                BIGINT,
	files_estimated           BIGINT,
	errors                    BIGINT,
	ignored_errors            BIGINT,
	current_path              TEXT,

	-- Object-storage traffic as tallied by the client's proxy to this point;
	-- the same four figures a completed run reports on backup_runs.
	s3_sent_raw_bytes         BIGINT,
	s3_sent_payload_bytes     BIGINT,
	s3_received_raw_bytes     BIGINT,
	s3_received_payload_bytes BIGINT,

	-- Engine-specific detail Canopy makes no commitment about: stored and
	-- displayed verbatim, never interpreted.
	extra                     JSONB NOT NULL DEFAULT '{}'
);

-- One run's series: the chart, and the latest sample for a run.
CREATE INDEX ON backup_run_progress (run_id, observed_at DESC);
-- Latest sample per run across a group, for the in-flight rows of the group view.
CREATE INDEX ON backup_run_progress (group_id, observed_at DESC);
-- Pruning.
CREATE INDEX ON backup_run_progress (observed_at);

-- When the run froze the data it captured — the point in time the backup
-- represents, as distinct from when its upload finished (reported_at). For a
-- large backup those are hours apart, and the freeze often happens below the
-- backup engine (a filesystem-level snapshot), so it leaves no trace in the
-- repository and has to come from the device. Written once per run: the first
-- value seen stands, whether it arrived on a progress sample or on the report.
-- Nullable, so clients that don't report it leave it unset and staleness falls
-- back to reported_at.
ALTER TABLE backup_runs ADD COLUMN snapshot_taken_at TIMESTAMPTZ;
