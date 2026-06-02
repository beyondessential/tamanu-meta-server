-- Server URLs are no longer unique, and are now optional. A server may be
-- identified solely by its bound device (e.g. a Tailscale node), with the URL
-- derived for display from the tailnet hostname.
DROP INDEX servers_host_live;
ALTER TABLE servers ALTER COLUMN host DROP NOT NULL;
-- Keep a plain (non-unique) index for the occasional host lookup.
CREATE INDEX servers_host ON servers (host) WHERE host IS NOT NULL;
