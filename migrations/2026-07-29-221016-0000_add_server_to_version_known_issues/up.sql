-- Which deployment's data provoked the issue; null when an operator filed it by
-- hand. SET NULL rather than CASCADE: the issue is about the version, and
-- outlives the server it was found on.
ALTER TABLE version_known_issues ADD COLUMN server_id UUID REFERENCES servers (id) ON DELETE SET NULL;
