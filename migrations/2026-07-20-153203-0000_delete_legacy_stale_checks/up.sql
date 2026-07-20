-- Per-source staleness (`stale/<source>` checks under the canopy source)
-- was folded into the single reachability check. Retire what's left:

-- Deactivate the open stale/<source> issues so they stop counting. Keep
-- the rows: they may be linked to past incidents, and being warnings they
-- never held an incident open, so deactivating them strands nothing.
update issues
set active = false
where source = 'canopy' and ref like 'stale/%' and active = true;

-- Drop the dead catalog entries so they no longer appear in the
-- healthcheck catalog. No foreign keys reference these.
delete from check_policies
where source = 'canopy' and check_name like 'stale/%';
