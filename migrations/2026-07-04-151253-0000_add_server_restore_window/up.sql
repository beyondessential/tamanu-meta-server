-- A per-server, time-boxed window during which the server may mint restore
-- credentials for itself (ad-hoc `bestool canopy restore`). An operator opens
-- the window; it auto-expires. NULL means restores are not currently allowed.
alter table servers
	add column restore_allowed_until timestamptz,
	add column restore_allowed_by text;
