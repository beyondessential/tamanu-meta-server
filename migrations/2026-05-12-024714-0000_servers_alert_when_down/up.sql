-- Opt-in for the canopy reachability sweep. The sweep observes every
-- server's latest status and files a (source=canopy, ref=reachability)
-- issue when the row gets stale. Servers we expect to go offline (test
-- environments, demos) set this to false and the sweep skips them — we
-- never collect those events in the first place, so it's "don't observe"
-- rather than "observe but mute".
--
-- Two-step add so the rollout is safe:
-- 1. Backfill every existing row with FALSE: turning the sweep on
--    globally at migration time would file Critical issues against every
--    server that's silently been offline (test boxes, demo VMs, dead
--    deployments we forgot about) and open incidents for the lot of
--    them. Operators flip the flag on per server after eyeballing the
--    inventory.
-- 2. Flip the column default to TRUE so freshly-registered servers DO
--    get the sweep applied — that's the safer default for the next
--    server we actually care about.
ALTER TABLE servers ADD COLUMN alert_when_down BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE servers ALTER COLUMN alert_when_down SET DEFAULT TRUE;
