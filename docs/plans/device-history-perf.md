# device-history-perf

The device detail page's "past server associations" section, and the first load of
old `/servers/<uuid>` pages, are very slow in production (>2 minutes). EXPLAIN
ANALYZE on prod confirmed the cause: queries against the weekly-partitioned
`statuses` and `device_connections` tables that have no `created_at` filter, so
partition pruning never engages and every weekly partition (89 / 61) is scanned.

For one tested device, `SELECT DISTINCT server_id FROM statuses WHERE device_id = ?`
took 143s and read ~3.4 GB of heap pages to return a single row.

## Fixes

Three changes, one commit each, in this order so each can be deployed independently
if needed.

### 1. Bound `Status::get_past_server_ids` by `created_at`

Add `AND created_at >= NOW() - INTERVAL '90 days'` to the query in
`crates/database/src/statuses.rs`. Lets partition pruning shrink the scan from
89 partitions to ~13. Smallest possible change, immediate hotfix value.

The semantics of the API's "past server associations" change from "ever seen"
to "seen in the last 90 days". This is fine for the UI's purpose, and is
superseded entirely by fix 3 below.

### 2. Bound `DeviceConnection::get_latest_from_device_ids` by `created_at`

Same pattern. Used by `Device::get_with_info` (which both the device page and
the server detail page hit) to render "last seen" / latest IP / user agent.
A 90-day bound is similarly safe — the value rendered is a recency indicator,
not an audit log.

### 3. Denormalised `device_server_associations` table

Long-term proper fix for past associations — bounds buy time but the underlying
problem is that we're aggregating a write-heavy fact table to answer a read
that should be O(1).

- New table `device_server_associations(device_id, server_id, first_seen, last_seen)`
  with `PRIMARY KEY (device_id, server_id)` and FKs to both parents.
- `AFTER INSERT` row trigger on `statuses` upserts into it whenever
  `device_id IS NOT NULL`. Triggers on partitioned tables propagate to all
  partitions automatically (PG13+), so future partitions inherit it.
- Backfill in the migration: `INSERT … SELECT device_id, server_id,
  MIN(created_at), MAX(created_at) FROM statuses GROUP BY 1,2`. Single full
  scan, expected on the order of minutes for prod — runs at deploy time.
- Switch `get_past_server_associations` (`crates/private-server/src/fns/devices.rs`)
  to read from the new table directly.
- Delete `Status::get_past_server_ids` — no other callers.

## Audit (after the three fixes)

User asked for a sweep of other queries with the same shape (read against a
partitioned table with no `created_at` bound). Candidates to check:
`connection_count`, `connection_history`, anything else hitting `statuses` or
`device_connections` without a time filter. Findings get their own commits.
