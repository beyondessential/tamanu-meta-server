CREATE TABLE artifacts (
	id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	version_id UUID NOT NULL REFERENCES versions(id) ON DELETE CASCADE,
	artifact_type TEXT NOT NULL,
	platform TEXT NOT NULL,
	download_url TEXT NOT NULL
);

ALTER TABLE artifacts ADD CONSTRAINT artifacts_type_platform_version_id UNIQUE (
    artifact_type, platform, version_id
);
SELECT diesel_manage_updated_at('artifacts');
