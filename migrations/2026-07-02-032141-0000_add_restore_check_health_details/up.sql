-- Arbitrary health data a restore consumer (e.g. PGRO) sends alongside a
-- restore check: postgres cluster stats, whether indexes needed fixing, etc.
-- Opaque to canopy for now (stored and displayed as-is); specific fields can be
-- promoted to typed columns later if they need to drive alerts or filtering.
ALTER TABLE backup_restore_checks ADD COLUMN health_details jsonb;
