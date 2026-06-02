-- Hybrid cache for a group card's headline version: the last reported version
-- of the group's canonical member (highest rank, then highest kind). Replaces a
-- per-render unbounded query that fanned out across every weekly partition of
-- the partitioned `statuses` table.
--
-- `version_server_id` is the canonical member whose version is cached;
-- `effective_version` is that member's last version-bearing status.

ALTER TABLE server_groups
    ADD COLUMN version_server_id UUID REFERENCES servers(id) ON DELETE SET NULL,
    ADD COLUMN effective_version TEXT;

CREATE INDEX server_groups_version_server_id ON server_groups (version_server_id);

-- AFTER INSERT trigger on statuses: when the canonical member reports a new
-- version, push it into the cache. No rank/kind logic — membership/rank changes
-- are handled by the app-level recompute. Only version-bearing statuses update
-- it, so a down/error (NULL-version) status never blanks the cached version.
CREATE OR REPLACE FUNCTION update_server_group_effective_version() RETURNS TRIGGER
LANGUAGE plpgsql AS $$
BEGIN
    IF NEW.version IS NOT NULL THEN
        UPDATE server_groups
        SET effective_version = NEW.version, updated_at = now()
        WHERE version_server_id = NEW.server_id;
    END IF;
    RETURN NEW;
END;
$$;

-- Row-level triggers on a partitioned parent propagate to all current and
-- future partitions in PG13+, so cron-created weekly partitions inherit it.
CREATE TRIGGER statuses_update_server_group_effective_version
    AFTER INSERT ON statuses
    FOR EACH ROW
    EXECUTE FUNCTION update_server_group_effective_version();

-- One-time backfill: for each group, pick the canonical member (lowest rank
-- priority, then kind priority, tie-broken by id) and cache its last
-- version-bearing status. Priority mappings mirror the Rust helpers.
WITH canonical AS (
    SELECT DISTINCT ON (group_id)
        group_id,
        id AS server_id
    FROM servers
    WHERE group_id IS NOT NULL
    ORDER BY
        group_id,
        CASE rank
            WHEN 'production' THEN 0
            WHEN 'clone' THEN 1
            WHEN 'demo' THEN 2
            WHEN 'test' THEN 3
            WHEN 'dev' THEN 4
            ELSE 5
        END,
        CASE kind
            WHEN 'central' THEN 0
            WHEN 'facility' THEN 1
            WHEN 'canopy' THEN 2
            ELSE 3
        END,
        id
)
UPDATE server_groups g
SET version_server_id = c.server_id,
    effective_version = (
        SELECT version
        FROM statuses
        WHERE server_id = c.server_id AND version IS NOT NULL
        ORDER BY created_at DESC
        LIMIT 1
    )
FROM canonical c
WHERE g.id = c.group_id;
