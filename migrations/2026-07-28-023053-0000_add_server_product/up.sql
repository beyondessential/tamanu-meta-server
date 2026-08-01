-- A server's product: which application it runs. `kind` remains the server's
-- role within that product's topology.
--
-- No CHECK constraint, matching how `kind` is stored: the valid set is code's
-- to define, so adding a product stays a code-only change.
ALTER TABLE servers ADD COLUMN product TEXT NOT NULL DEFAULT 'tamanu';

CREATE INDEX servers_product ON servers (product);

-- Canopy instances were classified by kind, the one product that had smuggled
-- itself onto that axis. Lift them onto `product` and leave `kind` as it is:
-- the kind column's role is being split over two releases, and rewriting it
-- here would leave a not-yet-upgraded binary unable to read these rows at all.
-- A later migration normalises the leftover `kind = 'canopy'` values.
UPDATE servers SET product = 'canopy' WHERE kind = 'canopy';
