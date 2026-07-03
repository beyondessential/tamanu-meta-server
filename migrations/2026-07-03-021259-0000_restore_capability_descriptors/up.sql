-- Capability descriptors (RST): a restore consumer advertises, per intent, a
-- human description, the Canopy-defined semantics it opts into, and a typed
-- parameter schema. Replicas carry the operator's parameter values, and the
-- overdue bound is renamed to reflect that its meaning is per-semantics (a
-- verify-once snapshot bound, or a standing replica's staleness bound).

ALTER TABLE restore_consumer_capabilities
	ADD COLUMN description TEXT,
	-- Canopy-defined behaviours the intent opts into (e.g. check, once, url), as
	-- a JSON string array; unrecognised entries are preserved but change no
	-- behaviour.
	ADD COLUMN semantics JSONB NOT NULL DEFAULT '[]'::jsonb,
	-- Parameter schema: name -> {type, default?}. Collected on the declaration
	-- form, validated, and passed through to the consumer in the worklist.
	ADD COLUMN params JSONB NOT NULL DEFAULT '{}'::jsonb;

-- Operator-supplied parameter values for the replica: name -> value. Only the
-- values the operator set; unset parameters are resolved to their default or
-- JSON null at worklist time.
ALTER TABLE restore_replicas
	ADD COLUMN params JSONB NOT NULL DEFAULT '{}'::jsonb;

-- The overdue bound. For a `once` intent it bounds how long the latest snapshot
-- may go unverified; otherwise it bounds staleness since the last healthy
-- report. NULL = no overdue bound.
ALTER TABLE restore_replicas RENAME COLUMN freshness TO overdue_after;
