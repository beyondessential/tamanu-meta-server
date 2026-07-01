-- Restore the untrusted default. The reassigned rows cannot be identified after
-- the fact, so this is a one-way data change; only the column default returns.
ALTER TABLE devices ALTER COLUMN role SET DEFAULT 'untrusted';
