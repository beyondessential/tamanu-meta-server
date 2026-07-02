# canopy-mcp plugin

Points agents at the internet-facing mount of Canopy's read-only MCP fleet
query interface (`https://meta.tamanu.app/mcp`). The `.mcp.json` carries no
credentials: the endpoint requires a bearer token, which must be supplied by
the agent platform (for Claude Tag, the admin Connections credential proxy).

Setup for Claude Tag (admin, at claude.ai/admin-settings/claude-tag):

1. Mint a token in the Canopy operator UI: Settings → MCP access. The
   `canopy_mcp_…` secret is shown once. Tokens expire after one year; a
   rotation alert fires 15 days out.
2. In the Access bundle's Credentials tab, use **Connect another app**:
   credential type **Bearer**, paste the secret, allowed website
   `meta.tamanu.app`.
3. Add this repository as an organization plugin source and attach the
   `canopy-mcp` plugin to the bundle.
4. Verify by asking Claude to run the `fleet_summary` tool; the token's
   "Last used" column in Canopy's Settings → MCP access updates within a
   minute.

Operators on the tailnet don't need this plugin or a token — use the tailnet
mount instead: `claude mcp add --transport http canopy
https://canopy.tail53aef.ts.net/api/mcp`.
