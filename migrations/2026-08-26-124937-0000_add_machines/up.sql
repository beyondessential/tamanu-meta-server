-- The machine grain: a host, as distinct from the application server on it.
--
-- Canopy's `server` conflated a box with the workload on it. That holds only
-- while one machine runs exactly one workload, and it already does not: some
-- Linux hosts run two application workloads today, and machine-level facts
-- (platform, OS, uptime, CPU, memory, filesystems, addresses) have nowhere to
-- go but whichever server row happens to be there.
--
-- `applications` was the old `servers`. This adds the machine beside it and
-- makes every application point at one. See [FLT](.workhorse/specs/servers/overview.md).
--
-- WHAT LANDS ON WHICH GRAIN. A machine carries the name its operator gave it,
-- its identity, its group, where it is, and how long it may be silent before
-- it is unreachable. An application carries its type, rank, name, public name,
-- URL, the DNS names it serves at, and its own silence threshold. Both carry
-- notes, tags, an archival flag and a monitoring switch, because a check can
-- be filed against either and graded by policy against its own target's tags.
--
-- THIS MIGRATION IS ADDITIVE ON `applications`. The machine-ish columns
-- (`device_id`, `cloud`, `geolocation`, `group_id`, `registered_at`) are
-- copied onto the machine and LEFT IN PLACE, so nothing that reads them breaks
-- while the machine grain is still being wired up. Dropping them is a later
-- step, once every reader has moved. `group_id` in particular stays for good
-- as a denormalisation, kept honest by a trigger.
--
-- MIGRATION. Every existing application is 1:1 with a machine, so the backfill
-- is mechanical: one machine per application, carrying that application's
-- machine-ish values and timestamps. With one application per machine there is
-- nothing to reconcile.

CREATE TABLE machines (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

	-- Operator-given. A machine's own hostname as the OS reports it is a
	-- reported figure, not this.
	name TEXT,

	-- The group is the only thing an operator supplies when creating a
	-- machine: which deployment a box belongs to is the one fact the box has
	-- no way of knowing.
	group_id UUID REFERENCES server_groups (id) ON UPDATE CASCADE ON DELETE SET NULL,

	-- A machine has at most one identity, and an identity belongs to at most
	-- one machine, so resolving one from the other is unambiguous.
	device_id UUID REFERENCES devices (id) ON UPDATE CASCADE ON DELETE SET NULL,

	cloud BOOLEAN,
	geolocation DOUBLE PRECISION[],

	alert_when_down_for INTERVAL NOT NULL DEFAULT '00:10:00'::interval,
	is_monitored BOOLEAN NOT NULL DEFAULT TRUE,

	notes TEXT NOT NULL DEFAULT ''::text,
	tags JSONB NOT NULL DEFAULT '{}'::jsonb,

	-- Archived rather than deleted: the record and its history remain.
	deleted_at TIMESTAMPTZ,
	-- When the box was enrolled. Also the anchor a backup deadline counts
	-- from, which is why it belongs to the machine and not to an application:
	-- adding a second workload to a box must not restart its backup clock.
	registered_at TIMESTAMPTZ,

	CONSTRAINT machines_alert_when_down_for_check
		CHECK (alert_when_down_for > '00:00:00'::interval),
	CONSTRAINT machines_tags_check
		CHECK (jsonb_typeof(tags) = 'object'::text)
);

-- Mirrors the equivalents on `applications`.
CREATE UNIQUE INDEX machines_device_id_unique ON machines (device_id) WHERE device_id IS NOT NULL;
CREATE INDEX machines_device ON machines (device_id);
CREATE INDEX machines_group_id ON machines (group_id) WHERE group_id IS NOT NULL;
CREATE INDEX machines_live ON machines (deleted_at) WHERE deleted_at IS NULL;

SELECT diesel_manage_updated_at('machines');

-- One machine per existing application, 1:1, keeping the application's id as
-- the machine's so the two are trivially correlatable while the split is
-- half-landed. They diverge as soon as a second application joins a machine.
--
-- `notes` and `tags` are NOT copied. An operator wrote them against the thing
-- they were managing, which becomes the application; duplicating them onto the
-- machine would mean two copies of one note drifting apart, and a policy rule
-- matching a tag twice over. A machine starts with neither, and anything that
-- turns out to be a fact about the box gets moved by hand.
INSERT INTO machines (
	id, created_at, updated_at, name, group_id, device_id, cloud, geolocation,
	alert_when_down_for, is_monitored, notes, tags, deleted_at, registered_at
)
SELECT
	a.id, a.created_at, a.updated_at, a.name, a.group_id, a.device_id, a.cloud,
	a.geolocation, a.alert_when_down_for, a.is_monitored, ''::text, '{}'::jsonb,
	a.deleted_at, a.registered_at
FROM applications a;

-- An application runs on exactly one machine.
ALTER TABLE applications
	ADD COLUMN machine_id UUID REFERENCES machines (id) ON UPDATE CASCADE ON DELETE CASCADE;

UPDATE applications SET machine_id = id;

ALTER TABLE applications ALTER COLUMN machine_id SET NOT NULL;

CREATE INDEX applications_machine_id ON applications (machine_id);

-- TRANSITION SCAFFOLDING. Remove when reports create applications.
--
-- An application inserted without a machine gets one of its own. That is
-- exactly the 1:1 the backfill above just performed, and exactly what the
-- model meant before the split: today the only thing that creates an
-- application is the operator "add a server" flow, which under the new model
-- is really creating a box.
--
-- This exists so the constraint can be NOT NULL from the outset rather than
-- nullable-and-tightened-later, which would push `Option<Uuid>` through every
-- reader and invite a bad default at each one.
--
-- THE HAZARD it carries: a caller that should attach an application to an
-- EXISTING machine, but omits `machine_id`, silently gets a second machine
-- instead of an error. That is wrong for a two-workload host, which is the
-- whole point of the card. So this default is removed in the same step that
-- makes reports create applications against a named machine (see [FLT], "Applications
-- come from reports"); until then, no such caller exists.
CREATE FUNCTION application_default_machine() RETURNS UUID
LANGUAGE sql VOLATILE AS $$
	INSERT INTO machines DEFAULT VALUES RETURNING id;
$$;

ALTER TABLE applications ALTER COLUMN machine_id SET DEFAULT application_default_machine();
