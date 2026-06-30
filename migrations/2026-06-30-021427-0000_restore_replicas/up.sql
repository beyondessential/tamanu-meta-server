-- Managed restore replicas (RST): operator-declared desired replicas that a
-- restore consumer reconciles against, plus the set of intents each consumer
-- can satisfy.

-- A declared replica: the operator's statement that a consumer should keep a
-- replica of a (group, [server | all servers], type) for a given intent. The
-- declaration is both the work item and the authorization to read what it
-- needs.
CREATE TABLE restore_replicas (
	id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	consumer_device_id UUID NOT NULL REFERENCES devices(id),
	group_id           UUID NOT NULL REFERENCES server_groups(id),
	-- NULL = all current servers in the group (expanded at worklist time).
	server_id          UUID REFERENCES servers(id),
	type               TEXT NOT NULL,
	intent             TEXT NOT NULL,
	name               TEXT NOT NULL,
	-- Max age of the restored snapshot before the replica is overdue; NULL =
	-- always track the latest snapshot.
	freshness          INTERVAL,
	enabled            BOOLEAN NOT NULL DEFAULT TRUE,
	created_by         TEXT,
	created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('restore_replicas');

-- One declaration per (consumer, group, type, intent) scope. A server-specific
-- row and a group-wide (server_id NULL) row are tracked under separate partial
-- indexes because NULLs do not compare equal in a plain unique constraint.
CREATE UNIQUE INDEX restore_replicas_scope_server
	ON restore_replicas (consumer_device_id, group_id, type, intent, server_id)
	WHERE server_id IS NOT NULL;
CREATE UNIQUE INDEX restore_replicas_scope_group
	ON restore_replicas (consumer_device_id, group_id, type, intent)
	WHERE server_id IS NULL;

CREATE INDEX restore_replicas_consumer ON restore_replicas (consumer_device_id);
CREATE INDEX restore_replicas_group ON restore_replicas (group_id);

-- The set of intents a consumer can satisfy, registered by the consumer on
-- start and whenever it changes. Canopy dispatches only matching worklist
-- entries and constrains the declaration UX to this set; an enabled
-- declaration whose intent is absent here is a surfaced gap.
CREATE TABLE restore_consumer_capabilities (
	consumer_device_id UUID NOT NULL REFERENCES devices(id),
	intent             TEXT NOT NULL,
	registered_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
	PRIMARY KEY (consumer_device_id, intent)
);
