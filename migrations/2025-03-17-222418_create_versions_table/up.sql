CREATE TABLE versions (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	major INT4 NOT NULL,
	minor INT4 NOT NULL,
	patch INT4 NOT NULL,
	published BOOLEAN NOT NULL DEFAULT true,
	changelog TEXT NOT NULL DEFAULT ''
);
ALTER TABLE versions ADD CONSTRAINT versions_version_number UNIQUE (
	major, minor, patch
);
SELECT diesel_manage_updated_at('versions');
