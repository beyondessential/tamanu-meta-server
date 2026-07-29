-- Server names and certificates (CRT): the addresses Canopy publishes on a
-- server's behalf, and the certificates it obtains for that server's names.
--
-- FK note: servers are archived (soft-deleted), never hard-deleted, so these
-- FKs are plain REFERENCES like the other server-scoped tables.

-- A name a server should be reachable at, with the addresses it reported and
-- the addresses Canopy has actually published there. The two are kept apart so
-- the reconcile can tell whether the zone already matches the intent, and so
-- Canopy only ever changes records it put there itself.
CREATE TABLE server_names (
	id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	server_id           UUID NOT NULL REFERENCES servers(id),
	-- Normalised: lower case, no trailing dot.
	name                TEXT NOT NULL,
	addresses           INET[] NOT NULL DEFAULT '{}',
	published_addresses INET[] NOT NULL DEFAULT '{}',
	published_at        TIMESTAMPTZ,
	-- Why the last publish attempt failed, cleared on success.
	last_error          TEXT,
	created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('server_names');

-- A name's addresses are registered by one server at a time, so two members of
-- a group cannot fight over where one name points.
CREATE UNIQUE INDEX server_names_name ON server_names (name);
CREATE INDEX server_names_server ON server_names (server_id);
-- The reconcile's work list: names whose published state doesn't match intent.
CREATE INDEX server_names_unpublished ON server_names (updated_at)
	WHERE addresses <> published_addresses;

-- A certificate order and, once it succeeds, the certificate it produced. One
-- row per (name, certified key): a repeat request for a key Canopy already has a
-- certificate for is answered from this row rather than ordering again, while a
-- request naming a different key is a different row and a new order.
--
-- The submitted CSR is kept because renewal reuses it: the key has not changed,
-- so Canopy can renew without asking the server for anything.
CREATE TABLE server_certificates (
	id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	server_id       UUID NOT NULL REFERENCES servers(id),
	-- Normalised: lower case, no trailing dot. Exactly one name per certificate.
	name            TEXT NOT NULL,
	-- Hex SHA-256 over the subject public key info of the key being certified.
	key_fingerprint TEXT NOT NULL,
	csr             BYTEA NOT NULL,
	state           TEXT NOT NULL CHECK (state IN ('pending', 'issued', 'failed')),
	-- The issued chain, PEM. Public material; the private key is never here,
	-- Canopy having never held one.
	chain           TEXT,
	not_after       TIMESTAMPTZ,
	issued_at       TIMESTAMPTZ,
	-- Set while the current order is extending a certificate that already
	-- issued, so a renewal failure is told apart from a first issuance that
	-- never came up.
	renewing        BOOLEAN NOT NULL DEFAULT false,
	attempts        INTEGER NOT NULL DEFAULT 0,
	next_attempt_at TIMESTAMPTZ NOT NULL DEFAULT now(),
	last_error      TEXT,
	created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
	updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

SELECT diesel_manage_updated_at('server_certificates');

CREATE UNIQUE INDEX server_certificates_name_key
	ON server_certificates (name, key_fingerprint);
CREATE INDEX server_certificates_server ON server_certificates (server_id);
-- The worker's claim query: orders due for an attempt, soonest first.
CREATE INDEX server_certificates_due ON server_certificates (next_attempt_at)
	WHERE state = 'pending';
-- The renewal sweep and the expiry alert both walk issued certificates by when
-- they run out.
CREATE INDEX server_certificates_expiry ON server_certificates (not_after)
	WHERE state = 'issued';
