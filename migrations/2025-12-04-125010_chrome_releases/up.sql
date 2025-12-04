CREATE TABLE chrome_releases (
	version TEXT PRIMARY KEY,
	release_date TEXT NOT NULL,
	is_eol BOOLEAN NOT NULL,
	eol_from TEXT,
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('chrome_releases');
CREATE INDEX chrome_releases_eol ON chrome_releases (is_eol);
