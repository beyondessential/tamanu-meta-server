-- An application has one type, which says what it is: the software and the
-- role it plays together.
--
-- `product` and `kind` were both approximations of this. Canopy had one record
-- per box, so it needed a field for which software ran there and another for
-- that software's role, and between them they described what is now simply the
-- application's type. A Tamanu central and a Tamanu facility are not one type
-- in two configurations — a large set of checks exists only on centrals and
-- another only on facilities — so they become two types.
ALTER TABLE applications ADD COLUMN type TEXT;

-- The pair maps together. Every product bar Tamanu had one role, `standalone`,
-- which was only ever the absence of a kind, so the software alone names it.
UPDATE applications SET type = CASE
	WHEN product = 'tamanu' AND kind = 'facility' THEN 'tamanu-facility'
	WHEN product = 'tamanu' THEN 'tamanu-central'
	ELSE product
END;

-- A row whose product was never one Canopy defined has nothing to map to. The
-- column is NOT NULL from here, so those are named rather than silently
-- defaulted to something they are not: the migration fails and whoever runs it
-- decides, which is right for data nobody knew was there.
DO $$
DECLARE unmapped TEXT;
BEGIN
	SELECT string_agg(DISTINCT product || '/' || kind, ', ')
	INTO unmapped
	FROM applications
	WHERE type NOT IN ('tamanu-central', 'tamanu-facility', 'senaite', 'canopy');

	IF unmapped IS NOT NULL THEN
		RAISE EXCEPTION 'applications carry product/kind pairs with no type: %', unmapped;
	END IF;
END $$;

ALTER TABLE applications ALTER COLUMN type SET NOT NULL;
ALTER TABLE applications ALTER COLUMN type SET DEFAULT 'tamanu-central';

ALTER TABLE applications DROP COLUMN product;
ALTER TABLE applications DROP COLUMN kind;

CREATE INDEX applications_type ON applications (type);
