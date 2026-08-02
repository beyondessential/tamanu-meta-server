-- Re-derive each group's canonical member, repairing what the original
-- backfill in 2026-06-02-071412 got wrong.
--
-- That backfill's rank `CASE` claimed to mirror the Rust helpers but listed
-- only the canonical spellings, sending the `live` / `prod` / `staging`
-- aliases — which `ServerRank::from_str` accepts, and which the pre-2026
-- `latest_statuses` view explicitly handled, so they really are in the data —
-- to the lowest-priority `ELSE` bucket. It also selected among *all* servers
-- rather than live ones, so an archived box could speak for its group. A
-- group whose central server is `rank='live'` beside a `rank='test'` box got
-- the test server as its `version_server_id`.
--
-- The trigger installed alongside it carries no rank logic, and the
-- app-level `ServerGroup::recompute_version` has always been correct. So the
-- damage is confined to groups that haven't been recomputed since June — but
-- for those it persists, and unlike a lost column it is fully recoverable
-- from current data.
--
-- Mirrors `ServerGroup::recompute_version` as it stands:
--   * live members only (`deleted_at IS NULL`),
--   * only products canopy holds a release train for (`tracks_versions`,
--     which today is tamanu alone — a group of others has no headline
--     version rather than a meaningless one),
--   * ordered by rank priority, then kind priority, tie-broken by id,
--   * version read from `server_reported_detail`, the current-state
--     projection that function uses, not from `statuses`.
WITH canonical AS (
    SELECT DISTINCT ON (group_id)
        group_id,
        id AS server_id
    FROM servers
    WHERE group_id IS NOT NULL
      AND deleted_at IS NULL
      AND product = 'tamanu'
    ORDER BY
        group_id,
        -- Same buckets as `rank_priority`, with every spelling
        -- `ServerRank::from_str` accepts folded onto its variant. An
        -- unrecognised or absent rank sorts last, as `None` does there.
        CASE LOWER(rank)
            WHEN 'production' THEN 0
            WHEN 'prod' THEN 0
            WHEN 'live' THEN 0
            WHEN 'clone' THEN 1
            WHEN 'staging' THEN 1
            WHEN 'demo' THEN 2
            WHEN 'test' THEN 3
            WHEN 'dev' THEN 4
            ELSE 5
        END,
        -- `kind_priority`. Note the original listed a `canopy` kind that no
        -- longer exists; the kinds are central, facility, standalone.
        CASE kind
            WHEN 'central' THEN 0
            WHEN 'facility' THEN 1
            WHEN 'standalone' THEN 2
            ELSE 3
        END,
        id
)
UPDATE server_groups g
SET version_server_id = c.server_id,
    effective_version = (
        SELECT d.version
        FROM server_reported_detail d
        WHERE d.server_id = c.server_id AND d.version IS NOT NULL
        ORDER BY d.reported_at DESC
        LIMIT 1
    )
FROM canonical c
WHERE g.id = c.group_id;

-- A group with no eligible member at all has no canonical member, and the
-- original backfill couldn't clear one it had wrongly set: its UPDATE only
-- touched groups the CTE matched.
UPDATE server_groups g
SET version_server_id = NULL,
    effective_version = NULL
WHERE g.version_server_id IS NOT NULL
  AND NOT EXISTS (
      SELECT 1
      FROM servers s
      WHERE s.group_id = g.id
        AND s.deleted_at IS NULL
        AND s.product = 'tamanu'
  );
