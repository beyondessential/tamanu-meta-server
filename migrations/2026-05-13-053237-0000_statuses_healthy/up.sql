-- Hoist `healthy` and `health` out of the free-form `extra` blob on
-- `statuses` so the public-server can read them cheaply and react to
-- transitions without parsing jsonb on every push.
--
-- Defaults preserve legacy behaviour: any status row predating this
-- migration, or any push that omits the new fields, reads back as
-- healthy with no per-check breakdown. The "absent ⇒ true" rule (per
-- the contract) is enforced by the column default plus the public
-- handler skipping the keys when serialising into `extra`.
ALTER TABLE statuses
	ADD COLUMN healthy BOOLEAN NOT NULL DEFAULT TRUE,
	ADD COLUMN health JSONB NOT NULL DEFAULT '[]'::jsonb;
