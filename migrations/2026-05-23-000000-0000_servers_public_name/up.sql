-- Replace `servers.listed` (boolean toggle for the public mobile list) with
-- `public_name` (nullable text). The mobile list now shows each server under
-- the operator-chosen public name, and NULL means "not listed". With server
-- groups in play, the server's internal `name` is no longer guaranteed to be
-- a useful global label, so this decouples the two.

ALTER TABLE servers ADD COLUMN public_name TEXT;

UPDATE servers
SET public_name = name
WHERE listed = TRUE AND name IS NOT NULL;

ALTER TABLE servers DROP COLUMN listed;
