-- `kind` was never rewritten, so Canopy instances are still identifiable by
-- `kind = 'canopy'` and nothing needs restoring before the column goes.
DROP INDEX servers_product;

ALTER TABLE servers DROP COLUMN product;
