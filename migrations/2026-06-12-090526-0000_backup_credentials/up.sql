-- Backup-credentials system: persistent state for the control plane that
-- issues short-lived S3 creds to devices and owns repo maintenance.
-- Backups are keyed (server, type): the repo is per-group/shared, the
-- backup *type* (e.g. tamanu-postgres) is a dimension on runs/issuances/
-- requests/snapshots. See docs/plans/backup-credentials.md.
--
-- FK note: server_groups / servers / devices are archived (soft-deleted),
-- never hard-deleted, so these FKs use plain REFERENCES (no ON DELETE /
-- ON UPDATE) — the cascade-vs-preserve distinction is moot in practice.

-- Repo-level backup config for a group (one bucket/repo per group, shared by
-- all the group's backup types). Schedule + retention live per-(group, type)
-- in server_group_backup_schedule, not here.
CREATE TABLE server_group_backup_config (
	group_id          UUID PRIMARY KEY REFERENCES server_groups(id),
	bucket            TEXT NOT NULL,
	prefix            TEXT NOT NULL DEFAULT '',
	target_role_arn   TEXT NOT NULL,
	region            TEXT,
	repo_password_ref TEXT NOT NULL,
	status            TEXT NOT NULL CHECK (status IN ('provisioning', 'escrow_pending', 'ready')),
	created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
SELECT diesel_manage_updated_at('server_group_backup_config');

-- Canopy-wide defaults per well-known backup type. auto_enable only seeds the
-- INITIAL enabled value on a capability at registration; it is not a perpetual
-- override.
CREATE TABLE backup_type_defaults (
	type              TEXT PRIMARY KEY,
	default_interval  INTERVAL,
	default_retention JSONB NOT NULL CHECK (jsonb_typeof(default_retention) = 'object'),
	auto_enable       BOOLEAN NOT NULL DEFAULT false
);

-- What each server ADVERTISES it can back up (bestool registers these), and
-- whether it's enabled for that server. enabled is SEEDED at registration from
-- the type's auto_enable default; thereafter it's operator-controlled state.
CREATE TABLE server_backup_capabilities (
	server_id     UUID NOT NULL REFERENCES servers(id),
	type          TEXT NOT NULL,
	enabled       BOOLEAN NOT NULL,
	registered_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (server_id, type)
);

-- Per-(group, type) schedule/retention OVERRIDES. Effective value =
-- this row ?? backup_type_defaults; org retention floor still applies (in
-- code). Absent row → the type defaults apply.
CREATE TABLE server_group_backup_schedule (
	group_id          UUID NOT NULL REFERENCES server_groups(id),
	type              TEXT NOT NULL,
	expected_interval INTERVAL,
	retention         JSONB CHECK (retention IS NULL OR jsonb_typeof(retention) = 'object'),
	created_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (group_id, type)
);
SELECT diesel_manage_updated_at('server_group_backup_schedule');

-- Audit log of every STS credential issuance. bucket/prefix are snapshots at
-- issuance time; access_key_id is the durable join to downstream CloudTrail.
CREATE TABLE backup_credential_issuances (
	id               BIGSERIAL PRIMARY KEY,
	device_id        UUID NOT NULL REFERENCES devices(id),
	group_id         UUID NOT NULL REFERENCES server_groups(id),
	type             TEXT NOT NULL,
	issued_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
	expires_at       TIMESTAMPTZ NOT NULL,
	purpose          TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
	sts_assumed_role TEXT NOT NULL,
	sts_request_id   TEXT,
	access_key_id    TEXT,
	bucket           TEXT NOT NULL,
	prefix           TEXT NOT NULL
);
CREATE INDEX ON backup_credential_issuances (device_id, issued_at DESC);
CREATE INDEX ON backup_credential_issuances (group_id, issued_at DESC);

-- What bestool reported per backup/restore run. id is the client-minted
-- run-uuid (stamped into the snapshot tags before this row exists), NOT a
-- serial — a duplicate id fails its own insert.
CREATE TABLE backup_runs (
	id             UUID PRIMARY KEY,
	device_id      UUID NOT NULL REFERENCES devices(id),
	group_id       UUID NOT NULL REFERENCES server_groups(id),
	server_id      UUID REFERENCES servers(id),
	type           TEXT NOT NULL,
	purpose        TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
	outcome        TEXT NOT NULL CHECK (outcome IN ('success', 'failure')),
	error          TEXT,
	bytes_uploaded BIGINT,
	snapshot_id    TEXT,
	reported_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX ON backup_runs (group_id, reported_at DESC);
CREATE INDEX ON backup_runs (device_id, reported_at DESC);
CREATE INDEX ON backup_runs (server_id, type, reported_at DESC);

-- Canopy-owned maintenance-Job outcomes (per-group / repo-level). outcome NULL
-- = still running.
CREATE TABLE backup_maintenance_runs (
	id              BIGSERIAL PRIMARY KEY,
	group_id        UUID NOT NULL REFERENCES server_groups(id),
	kind            TEXT NOT NULL CHECK (kind IN ('quick', 'full')),
	started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
	finished_at     TIMESTAMPTZ,
	outcome         TEXT CHECK (outcome IS NULL OR outcome IN ('success', 'failure')),
	error           TEXT,
	bytes_reclaimed BIGINT
);
CREATE INDEX ON backup_maintenance_runs (group_id, started_at DESC);

-- Ground-truth inventory from the read-only inspection Job: latest snapshot
-- per kopia source. source encodes the server id + type; (group, source) is
-- the upsert key.
CREATE TABLE backup_repo_snapshots (
	group_id           UUID NOT NULL REFERENCES server_groups(id),
	source             TEXT NOT NULL,
	server_id          UUID REFERENCES servers(id),
	type               TEXT,
	latest_snapshot_at TIMESTAMPTZ,
	observed_at        TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (group_id, source)
);

-- Cached repo + bucket size/stats for operator display (per-group). Filled by
-- two writers: the inspection Job (repo-derived fields) and the S3-metrics
-- task (bucket_bytes, best-effort/nullable).
CREATE TABLE backup_repo_stats (
	group_id       UUID PRIMARY KEY REFERENCES server_groups(id),
	snapshot_count INTEGER,
	source_count   INTEGER,
	logical_bytes  BIGINT,
	physical_bytes BIGINT,
	bucket_bytes   BIGINT,
	observed_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Pending operator one-off "backup now" flags, per (server, type, purpose);
-- cleared when the run is reported.
CREATE TABLE backup_requests (
	server_id    UUID NOT NULL REFERENCES servers(id),
	type         TEXT NOT NULL,
	purpose      TEXT NOT NULL CHECK (purpose IN ('backup', 'restore')),
	requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	requested_by TEXT,
	PRIMARY KEY (server_id, type, purpose)
);
