-- Reachability is now determined at the machine grain as well as the
-- application grain, and a silence follows the grain the check is filed at.
--
-- So an operator who silenced unreachability on a server before the split
-- silenced the only reachability filing that existed. After it, the box files
-- its own — a filing nothing has ever been asked about — and every deliberately
-- quiet host in the fleet opens an incident the moment the split lands. That is
-- not the operator changing their mind; it is their instruction losing the
-- thing it was attached to.
--
-- So carry it. A box is silenced when every live application on it was, which
-- for the 1:1 boxes this fleet is made of is the same set of hosts the operator
-- silenced, and for a shared box refuses to silence more than they asked for:
-- silencing one of two workloads was never a statement about the host.
--
-- A box with nothing live on it has nothing to derive an instruction from and
-- is left alone.
--
-- See CHK (.workhorse/specs/monitoring/checks.md), "Reachability".

INSERT INTO scoped_check_policies (
	created_at, updated_at, source, check_name,
	machine_id, ceiling, created_by, subject, application_type
)
SELECT
	min(silence.created_at),
	now(),
	'canopy',
	'reachability',
	app.machine_id,
	'skipped',
	-- The operator who silenced it first. The instruction is theirs, and the
	-- silence list is where an operator goes to ask whose it was.
	(array_agg(silence.created_by ORDER BY silence.created_at))[1],
	-- Canopy curates its own check names, so reachability is unqualified.
	NULL,
	NULL
FROM applications app
JOIN scoped_check_policies silence
	ON silence.application_id = app.id
	AND silence.source = 'canopy'
	AND silence.check_name = 'reachability'
	AND silence.ceiling = 'skipped'
WHERE app.deleted_at IS NULL
  AND app.id <> '00000000-0000-0000-0000-000000000000'
  AND NOT EXISTS (
	SELECT 1 FROM scoped_check_policies held
	WHERE held.machine_id = app.machine_id
	  AND held.source = 'canopy'
	  AND held.check_name = 'reachability'
  )
GROUP BY app.machine_id
HAVING count(*) = (
	SELECT count(*)
	FROM applications sibling
	WHERE sibling.machine_id = app.machine_id
	  AND sibling.deleted_at IS NULL
	  AND sibling.id <> '00000000-0000-0000-0000-000000000000'
);

-- The incidents already opened are left to close themselves. The reachability
-- sweep re-files an unreachable box on every pass, and the next one grades it
-- against the silence now in place, drops it from its incident, and closes the
-- incident along with the last of its members. Detaching them here would mean
-- restating incident membership in SQL, where it would be a second, silent copy
-- of a rule that lives in Rust.
