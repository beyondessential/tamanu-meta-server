CREATE UNIQUE INDEX bestool_snippets_unique_supersedes_id_idx 
ON bestool_snippets (supersedes_id) 
WHERE supersedes_id IS NOT NULL;
