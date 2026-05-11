-- Display metadata for Tailscale users — name and profile picture URL.
-- Populated lazily: handlers that need to record "this human did X"
-- upsert the row from the request's Tailscale headers, so the latest
-- copy is always available to render avatars in issue/incident views.

CREATE TABLE tailscale_users (
	login TEXT PRIMARY KEY,
	name TEXT NOT NULL,
	profile_pic TEXT,
	created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
	updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('tailscale_users');
