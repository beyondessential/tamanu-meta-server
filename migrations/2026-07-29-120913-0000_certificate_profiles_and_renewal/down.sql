DROP TABLE compromised_keys;

ALTER TABLE server_certificates
	DROP COLUMN revocation_reason,
	DROP COLUMN revoked_by,
	DROP COLUMN revoked_at;

ALTER TABLE server_certificates DROP CONSTRAINT server_certificates_state_check;
ALTER TABLE server_certificates ADD CONSTRAINT server_certificates_state_check
	CHECK (state IN ('pending', 'issued', 'failed'));

DROP INDEX server_certificates_renewal;

ALTER TABLE server_certificates
	DROP COLUMN renew_after,
	DROP COLUMN profile;

ALTER TABLE servers DROP COLUMN certificate_profile;
