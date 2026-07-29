-- Certificate profiles and authority-driven renewal (CRT).
--
-- An authority may offer several lifetimes, named as profiles. Which one a
-- server's certificates use is an operator's choice per server, because lifetime
-- is a property of how a deployment is run: a cloud deployment whose issuance is
-- exercised constantly can carry a short lifetime, where an on-premises one that
-- may be offline for days cannot.
--
-- NULL means "the longest profile the authority offers", which is every server
-- until an operator says otherwise — a short lifetime is adopted deliberately
-- rather than inherited.
ALTER TABLE servers ADD COLUMN certificate_profile TEXT;

-- The profile a certificate was actually issued under, which is not necessarily
-- the server's current choice: changing the choice takes effect at the next
-- renewal rather than invalidating what is held.
ALTER TABLE server_certificates ADD COLUMN profile TEXT;

-- When this certificate should next be considered for renewal.
--
-- Set from the authority's own renewal information where it publishes any, and
-- otherwise from a fixed fraction of the certificate's life. Stored rather than
-- computed per pass so the renewal sweep doesn't re-ask the authority about
-- every certificate it holds, and so a 6-day certificate and a 90-day one can be
-- swept by the same query.
ALTER TABLE server_certificates ADD COLUMN renew_after TIMESTAMPTZ;

-- The renewal sweep's work list. A certificate with no renew_after has not been
-- issued yet, so it is the order queue's business rather than this one's.
CREATE INDEX server_certificates_renewal ON server_certificates (renew_after)
	WHERE state = 'issued' AND renew_after IS NOT NULL;

-- Operator-initiated revocation. Canopy holds the account that obtained the
-- certificate, which is authority enough to revoke it: the server's private key
-- is neither needed nor asked for.
--
-- `revoked` joins the states rather than being a flag, because a revoked
-- certificate is not a certificate any more: it is not renewed, not collected,
-- and not counted as held.
ALTER TABLE server_certificates DROP CONSTRAINT server_certificates_state_check;
ALTER TABLE server_certificates ADD CONSTRAINT server_certificates_state_check
	CHECK (state IN ('pending', 'issued', 'failed', 'revoked'));

ALTER TABLE server_certificates
	ADD COLUMN revoked_at TIMESTAMPTZ,
	ADD COLUMN revoked_by TEXT,
	-- The RFC 5280 reason given, by name. `key_compromise` additionally bars the
	-- key from being certified again.
	ADD COLUMN revocation_reason TEXT;

-- A key revoked for compromise is never certified again, whatever asks for it.
-- Kept as its own table rather than inferred by scanning revoked certificates:
-- the check happens on every request, and the answer must not depend on a row
-- that could be tidied away later.
CREATE TABLE compromised_keys (
	key_fingerprint TEXT PRIMARY KEY,
	-- The certificate whose revocation barred it, for an operator tracing why.
	certificate_id  UUID REFERENCES server_certificates(id),
	noted_by        TEXT,
	noted_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
