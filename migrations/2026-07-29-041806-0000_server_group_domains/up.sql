-- Server group domains (DOM): which DNS names each group controls, and which
-- servers are trusted to manage their own names under them.
--
-- The zones Canopy can write records in are deployment configuration, not
-- state, so they have no table here: a claim is validated against the
-- configured zones at claim time and re-matched on every read, which is what
-- lets a claim survive its zone leaving the configuration (reported as
-- unmatched) instead of being silently actionable or silently dropped.
--
-- FK note: server_groups is archived (soft-deleted), never hard-deleted, so
-- this FK is a plain REFERENCES like the other group-scoped tables.

-- A domain a group controls, holding everything beneath it. Claims never
-- overlap fleet-wide (see database::server_domains::claim), so at most one
-- group controls any given name.
CREATE TABLE server_group_domains (
	id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	group_id   UUID NOT NULL REFERENCES server_groups(id),
	-- Normalised: lower case, no trailing dot, at least two labels.
	domain     TEXT NOT NULL,
	created_by TEXT,
	created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('server_group_domains');

-- Exact duplicates are impossible even under a concurrent claim; the wider
-- no-overlap rule (a claim above or below another) needs a read of neighbouring
-- rows, so claiming serialises on an advisory lock and checks there.
CREATE UNIQUE INDEX server_group_domains_domain ON server_group_domains (domain);
CREATE INDEX server_group_domains_group ON server_group_domains (group_id);

-- Whether this server may manage its own DNS records, and whether it may obtain
-- its own TLS certificates, for names under its group's domains. Withheld by
-- default: a server that has neither is authenticated and refused.
ALTER TABLE servers
	ADD COLUMN may_manage_dns BOOLEAN NOT NULL DEFAULT false,
	ADD COLUMN may_manage_tls BOOLEAN NOT NULL DEFAULT false;
