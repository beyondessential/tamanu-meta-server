ALTER TABLE versions
	ADD COLUMN device_id UUID REFERENCES devices(id);

ALTER TABLE artifacts
	ADD COLUMN device_id UUID REFERENCES devices(id);
