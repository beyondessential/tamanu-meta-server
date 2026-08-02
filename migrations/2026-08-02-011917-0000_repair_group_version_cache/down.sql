-- Nothing to undo. This migration only recomputes a cache from data that is
-- still present, so reverting it would mean restoring values that were wrong
-- — and the app recomputes the same answer on the next membership change
-- either way.
SELECT 1;
