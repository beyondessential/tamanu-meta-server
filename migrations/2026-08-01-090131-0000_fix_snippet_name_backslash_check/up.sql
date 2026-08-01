-- Reject a backslash in a snippet name, not a trailing percent sign.
--
-- Backslash is LIKE's default escape character, so the `name NOT LIKE '%\%'`
-- clause read as "does not end in a literal percent sign" — the opposite of
-- what the comment above it claimed. Verified on Postgres 16:
--   'foo\bar' LIKE '%\%'  -> false   (backslash names were accepted)
--   'top100%' LIKE '%\%'  -> true    (percent names were rejected)
-- while the intended '%\\%' gives true and false respectively.
--
-- Names become client-side filenames on Windows bestool, which is why
-- backslash is on the forbidden list; a percent sign was never meant to be.
--
-- Added NOT VALID: the corrected clause is enforced on every insert and
-- update from here on, but existing rows aren't scanned. A legacy name
-- containing a backslash — accepted for as long as this constraint has been
-- wrong — would otherwise fail the migration and block the deploy. Once the
-- fleet is known clean, `ALTER TABLE bestool_snippets VALIDATE CONSTRAINT
-- valid_snippet_name;` completes the job without taking a write lock.
ALTER TABLE bestool_snippets
	DROP CONSTRAINT valid_snippet_name;

ALTER TABLE bestool_snippets
	ADD CONSTRAINT valid_snippet_name CHECK (
		-- No spaces
		name NOT LIKE '% %' AND
		-- No forbidden special characters: . / < > : " ' \ | ? *
		name NOT LIKE '%.%' AND
		name NOT LIKE '%/%' AND
		name NOT LIKE '%<%' AND
		name NOT LIKE '%>%' AND
		name NOT LIKE '%:%' AND
		name NOT LIKE '%"%' AND
		name NOT LIKE '%''%' AND
		name NOT LIKE '%\\%' AND
		name NOT LIKE '%|%' AND
		name NOT LIKE '%?%' AND
		name NOT LIKE '%*%' AND
		-- No control characters (U+0000-U+001F) - match any character in range
		name !~ '[\x00-\x1f]' AND
		-- Not Windows reserved names (case-insensitive)
		LOWER(name) NOT IN (
			'con', 'prn', 'aux', 'nul',
			'com1', 'com2', 'com3', 'com4', 'com5', 'com6', 'com7', 'com8', 'com9',
			'lpt1', 'lpt2', 'lpt3', 'lpt4', 'lpt5', 'lpt6', 'lpt7', 'lpt8', 'lpt9'
		)
	) NOT VALID;
