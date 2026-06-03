-- Data backfill; nothing sensible to undo. The backfilled values are
-- indistinguishable from organically-set ones by design, and reverting
-- would reintroduce the "hasn't checked in yet" misreport.
SELECT 1;
