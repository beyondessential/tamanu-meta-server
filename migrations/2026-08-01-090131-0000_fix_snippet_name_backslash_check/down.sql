ALTER TABLE bestool_snippets
	DROP CONSTRAINT valid_snippet_name;

ALTER TABLE bestool_snippets
	ADD CONSTRAINT valid_snippet_name CHECK (
		name NOT LIKE '% %' AND
		name NOT LIKE '%.%' AND
		name NOT LIKE '%/%' AND
		name NOT LIKE '%<%' AND
		name NOT LIKE '%>%' AND
		name NOT LIKE '%:%' AND
		name NOT LIKE '%"%' AND
		name NOT LIKE '%''%' AND
		name NOT LIKE '%\%' AND
		name NOT LIKE '%|%' AND
		name NOT LIKE '%?%' AND
		name NOT LIKE '%*%' AND
		name !~ '[\x00-\x1f]' AND
		LOWER(name) NOT IN (
			'con', 'prn', 'aux', 'nul',
			'com1', 'com2', 'com3', 'com4', 'com5', 'com6', 'com7', 'com8', 'com9',
			'lpt1', 'lpt2', 'lpt3', 'lpt4', 'lpt5', 'lpt6', 'lpt7', 'lpt8', 'lpt9'
		)
	) NOT VALID;
