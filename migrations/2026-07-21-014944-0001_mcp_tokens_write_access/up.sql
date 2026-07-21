-- Whether the token may call the MCP write tools (manual incidents). A
-- mint-time choice, immutable afterwards; existing tokens stay read-only.
ALTER TABLE mcp_tokens ADD COLUMN write_access BOOLEAN NOT NULL DEFAULT FALSE;
