-- Restore-health reports: one row per report a consumer sends about a replica.
-- The strongest backup-health signal — proof a snapshot actually restored into
-- a healthy database (stronger than "a backup ran" or "a snapshot persisted").

CREATE TABLE backup_restore_checks (
	id                 BIGSERIAL PRIMARY KEY,
	-- The declaration this check concerns; nullable so history survives the
	-- declaration being retired.
	replica_id         UUID REFERENCES restore_replicas(id),
	consumer_device_id UUID NOT NULL REFERENCES devices(id),
	group_id           UUID NOT NULL REFERENCES server_groups(id),
	-- Nullable like backup_runs.server_id: a server may be archived after the
	-- snapshot it produced was restored.
	server_id          UUID REFERENCES servers(id),
	type               TEXT NOT NULL,
	intent             TEXT NOT NULL,
	-- The snapshot that was restored; null on a failure that never got that far.
	snapshot_id        TEXT,
	outcome            TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
	error              TEXT,
	replica_healthy    BOOLEAN NOT NULL,
	postgres_version   TEXT,
	observed_at        TIMESTAMPTZ NOT NULL,
	s3_sent_raw_bytes        BIGINT,
	s3_sent_payload_bytes    BIGINT,
	s3_received_raw_bytes    BIGINT,
	s3_received_payload_bytes BIGINT,
	reported_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX backup_restore_checks_group_type ON backup_restore_checks (group_id, type, observed_at DESC);
CREATE INDEX backup_restore_checks_server_type ON backup_restore_checks (server_id, type, observed_at DESC);
CREATE INDEX backup_restore_checks_snapshot ON backup_restore_checks (snapshot_id);
CREATE INDEX backup_restore_checks_replica ON backup_restore_checks (replica_id);
