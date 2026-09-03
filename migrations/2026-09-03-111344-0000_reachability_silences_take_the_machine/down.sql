-- Undo by the same rule `up` applied: a machine-scoped reachability silence
-- over a box whose every live application also carries one is a row this
-- migration would have created.
--
-- The rule is stated rather than exact. An operator who silenced such a box by
-- hand is indistinguishable from the migration having done it, so their row
-- goes too — which is the direction that errs toward the pre-migration shape,
-- and is recoverable with one switch.

DELETE FROM scoped_check_policies silence
WHERE silence.machine_id IS NOT NULL
  AND silence.source = 'canopy'
  AND silence.check_name = 'reachability'
  AND silence.ceiling = 'skipped'
  AND EXISTS (
	SELECT 1
	FROM applications app
	WHERE app.machine_id = silence.machine_id
	  AND app.deleted_at IS NULL
	  AND app.id <> '00000000-0000-0000-0000-000000000000'
  )
  AND NOT EXISTS (
	SELECT 1
	FROM applications app
	WHERE app.machine_id = silence.machine_id
	  AND app.deleted_at IS NULL
	  AND app.id <> '00000000-0000-0000-0000-000000000000'
	  AND NOT EXISTS (
		SELECT 1 FROM scoped_check_policies own
		WHERE own.application_id = app.id
		  AND own.source = 'canopy'
		  AND own.check_name = 'reachability'
		  AND own.ceiling = 'skipped'
	  )
  );
