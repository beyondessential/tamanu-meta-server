-- Fold the machine's detail back onto its applications and restore the single
-- table.
--
-- A machine's fields land on every application it hosts, which is the
-- duplication the split removed. Lossless for the 1:1 fleet the split started
-- from.

UPDATE application_reported_detail d
SET extra = m.extra || d.extra
FROM applications a, machine_reported_detail m
WHERE a.id = d.application_id AND m.machine_id = a.machine_id AND m.source = d.source;

DROP TABLE machine_reported_detail;

ALTER TABLE application_reported_detail
	RENAME CONSTRAINT application_reported_detail_application_id_fkey TO server_reported_detail_server_id_fkey;
ALTER INDEX application_reported_detail_pkey RENAME TO server_reported_detail_pkey;
ALTER TABLE application_reported_detail RENAME COLUMN application_id TO server_id;
ALTER TABLE application_reported_detail RENAME TO server_reported_detail;
