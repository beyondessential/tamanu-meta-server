-- Remove the valid_snippet_name check constraint
ALTER TABLE bestool_snippets
DROP CONSTRAINT valid_snippet_name;
