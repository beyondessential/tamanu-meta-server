ALTER TABLE issues ADD COLUMN severity TEXT NOT NULL DEFAULT 'info';
UPDATE issues SET severity = CASE
	WHEN effective_result = 'failed' AND escalates THEN 'critical'
	WHEN effective_result = 'failed' THEN 'error'
	WHEN effective_result IN ('warning', 'broken') THEN 'warning'
	ELSE 'info'
END;
