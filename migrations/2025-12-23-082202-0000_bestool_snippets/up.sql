CREATE TABLE bestool_snippets (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    created_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP,
    deleted_at TIMESTAMPTZ,
    supersedes_id UUID REFERENCES bestool_snippets(id),
    editor TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    sql TEXT NOT NULL
);

CREATE UNIQUE INDEX bestool_snippets_name_unique_active ON bestool_snippets(name) WHERE supersedes_id IS NULL AND deleted_at IS NULL;
