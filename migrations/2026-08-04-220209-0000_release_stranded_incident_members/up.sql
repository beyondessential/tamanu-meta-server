-- Release membership rows that outlived the incident they name.
--
-- Closing an incident never stamped `left_at` on the members that hadn't left
-- of their own accord, so a sub-failure contributor stayed attached to an
-- incident that had been closed for weeks. Those rows read as live
-- membership, which kept the issue from ever opening another incident: a
-- server could sit red with nothing paging.
--
-- The membership ended when the incident closed, so that is what `left_at`
-- becomes. Rows whose incident is still open are live and are left alone.

UPDATE incident_issues ii
SET left_at = i.closed_at
FROM incidents i
WHERE i.id = ii.incident_id
  AND ii.left_at IS NULL
  AND i.closed_at IS NOT NULL;
