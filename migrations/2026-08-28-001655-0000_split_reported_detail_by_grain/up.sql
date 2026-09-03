-- Reported detail splits by grain: the box's facts to the machine, the
-- workload's to the application.
--
-- One table keyed `(server_id, source)` becomes two, keyed `(machine_id,
-- source)` and `(application_id, source)`. A host running two workloads
-- reports its platform, memory and filesystems once rather than once per
-- workload, which is the whole point of the split.
--
-- `version` stays with the application and has no machine counterpart: a
-- version is what the workload runs. The machine's own agent version
-- (`bestoolVersion`) is a detail field like any other, and goes to the machine.
--
-- THE SPLIT LIST lives in `commons_types::subject`, beside the check-subject
-- list, because both answer the same question. It is repeated here only to
-- move the rows that already exist; the running split is Rust's.
--
-- READS ARE UNAFFECTED. Everything that reads a figure asks for an
-- application and gets its own detail merged with its machine's, exactly as it
-- did when both lived in one row. The storage is what changes.

ALTER TABLE server_reported_detail RENAME TO application_reported_detail;
ALTER TABLE application_reported_detail RENAME COLUMN server_id TO application_id;
ALTER INDEX server_reported_detail_pkey RENAME TO application_reported_detail_pkey;
ALTER TABLE application_reported_detail
	RENAME CONSTRAINT server_reported_detail_server_id_fkey TO application_reported_detail_application_id_fkey;

CREATE TABLE machine_reported_detail (
	machine_id UUID NOT NULL REFERENCES machines (id) ON DELETE CASCADE,
	source TEXT NOT NULL,
	extra JSONB NOT NULL DEFAULT '{}'::jsonb,
	reported_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
	PRIMARY KEY (machine_id, source)
);

-- `extra` is `NOT NULL`, which does not make it an object: JSON `null` is a
-- value, so `NOT NULL` admits it and the backfill's `COALESCE(extra, '{}')`
-- never saw it. Rows carrying it exist. Both statements below need an object —
-- `jsonb_each` refuses a non-object, and so does each `-` key delete — so
-- flatten anything that isn't one to the empty object it already means.
UPDATE application_reported_detail SET extra = '{}'::jsonb
WHERE jsonb_typeof(extra) <> 'object';

-- Move the box's fields off each application's row and onto its machine's.
--
-- A machine may host several applications, each with a row for the same
-- source, so the insert coalesces: last writer per `(machine, source)` wins,
-- which is the same rule the merged read applies anyway. With today's 1:1 that
-- never arises.
INSERT INTO machine_reported_detail (machine_id, source, extra, reported_at)
SELECT
	a.machine_id,
	d.source,
	jsonb_object_agg(e.key, e.value),
	MAX(d.reported_at)
FROM application_reported_detail d
JOIN applications a ON a.id = d.application_id
CROSS JOIN LATERAL jsonb_each(d.extra) AS e(key, value)
WHERE e.key IN (
	'arch', 'bestoolVersion', 'cpuCores', 'filesystems', 'hostname',
	'instanceTags', 'ipv4', 'ipv6', 'kernel', 'lanIps', 'munin', 'nat64',
	'osKind', 'osName', 'osTimezone', 'osVersion', 'services',
	'totalMemoryBytes', 'uptimeSecs', 'virtualisation', 'virtualised',
	'wanIpv4', 'wanIpv6'
)
GROUP BY a.machine_id, d.source
ON CONFLICT (machine_id, source) DO UPDATE
	SET extra = machine_reported_detail.extra || EXCLUDED.extra,
	    reported_at = GREATEST(machine_reported_detail.reported_at, EXCLUDED.reported_at);

-- And take them out of the application's row, so each fact is stored once.
UPDATE application_reported_detail SET extra = extra
	- 'arch' - 'bestoolVersion' - 'cpuCores' - 'filesystems' - 'hostname'
	- 'instanceTags' - 'ipv4' - 'ipv6' - 'kernel' - 'lanIps' - 'munin' - 'nat64'
	- 'osKind' - 'osName' - 'osTimezone' - 'osVersion' - 'services'
	- 'totalMemoryBytes' - 'uptimeSecs' - 'virtualisation' - 'virtualised'
	- 'wanIpv4' - 'wanIpv6';
