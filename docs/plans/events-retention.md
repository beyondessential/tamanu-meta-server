# Events retention & partitioning (draft)

**Status: draft.** `events` grows without bound — one row per issue
state-change and per contributing device push. This plan bounds it with
time-based partitioning and drop-by-partition retention.

## Sizing

Observed ≈ **6M rows/year**, extrapolated from the first ~1½ months of
`events` data. That's the low end of the theoretical range (worst case ≈ 100
devices × 1 push/minute with no coalescing ≈ 50M rows/year; steady-state
coalescing cuts that roughly 10×, though a flapping issue doesn't). Re-check
the rate as the fleet and event mix grow:

```sql
SELECT count(*) * EXTRACT(EPOCH FROM ('1 year'::interval))
              / EXTRACT(EPOCH FROM (max(created_at) - min(created_at)))
FROM events;
```

## Approach

1. **Partition `events` by `created_at`, weekly** — the same pattern
   `statuses` and `device_connections` already use (cron-maintained partition
   manager, proven in-tree; see the weekly-partition migrations).
2. **Retention by partition drop.** Keep N weeks (e.g. 26 ≈ 6 months), drop
   older partitions. The count is configurable.
3. **Indexes.** `(issue_id, created_at DESC)` within each partition; the "list
   events for an issue" query wants recent rows, so partition pruning helps.

## Non-goals

- Don't keep daily aggregates while dropping individual rows — that's a
  separate analytical concern. If long-term event-rate trends are wanted,
  ship them as a materialized view that survives the partition drops.

## Open question

- Partition `incident_issues` too? Probably not — it grows far slower (one row
  per join/leave, not per push). `incidents` likewise.

## Effort

Medium-small: the infrastructure exists in-repo, so it's copy-and-adapt from
the `statuses` partitioning. The retention number is an ops decision, not a
code one.
