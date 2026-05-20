ALTER TABLE issues ADD COLUMN acknowledged_at TIMESTAMP WITH TIME ZONE;
ALTER TABLE issues ADD COLUMN acknowledged_by TEXT;
ALTER TABLE incidents ADD COLUMN acknowledged_at TIMESTAMP WITH TIME ZONE;
ALTER TABLE incidents ADD COLUMN acknowledged_by TEXT;
