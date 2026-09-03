-- Back to a product and a kind. The split is lossless in both directions for
-- the closed set of types, since each type names exactly one software and one
-- role.
DROP INDEX applications_type;

ALTER TABLE applications ADD COLUMN product TEXT NOT NULL DEFAULT 'tamanu';
ALTER TABLE applications ADD COLUMN kind TEXT NOT NULL DEFAULT 'central';

UPDATE applications SET
	product = CASE
		WHEN type IN ('tamanu-central', 'tamanu-facility') THEN 'tamanu'
		ELSE type
	END,
	kind = CASE
		WHEN type = 'tamanu-central' THEN 'central'
		WHEN type = 'tamanu-facility' THEN 'facility'
		ELSE 'standalone'
	END;

ALTER TABLE applications DROP COLUMN type;

CREATE INDEX servers_product ON applications (product);
CREATE INDEX servers_kind ON applications (kind);
