# Remove pingtask and decommission related checks

Verification for removing canopy's outbound ping loop and the nil-UUID own-status loop.
The weight of the testing is on what must *keep* working: reachability is computed from pushed statuses alone, and the historical data pingtask produced is still there.

## Reachability survives without pingtask

These are covered by the existing sweep suite in `crates/database/tests/it/reachability_sweep.rs`, which drives the sweep from status rows and check states directly — never from a ping.

- [x] A server whose only source goes stale past its threshold files a failed reachability check (verifies spec: CHK)
- [x] A server that has never reported files a failed reachability check (verifies spec: CHK)
- [x] One `on` source stale while others still report warns and names the stale source in the detail (verifies spec: CHK)
- [x] Every source stale fails and presents the server as unreachable (verifies spec: CHK)
- [x] A stale `quiet` source raises no warning but still counts toward unreachable (verifies spec: CHK)
- [x] An `off` source, and a source not in ingest mode `allow`, are excluded from reachability (verifies spec: CHK)
- [x] Reachability closes when a source starts reporting again (verifies spec: CHK)
- [x] An unmonitored server still records reachability without alerting (verifies spec: CHK)
- [x] The per-server unreachability silence still reaches the reachability check from server settings (`private-web/e2e/server-reachability-silence.spec.ts`) (verifies spec: CHK)

## The nil-UUID server keeps its non-status roles

Only the status logging against the nil server goes; the row itself is the global-scope sentinel and the target for MCP token-expiry issues.

- [x] Global-scope issues resolve against the nil server id (`crates/database/tests/it/scope.rs`)
- [x] MCP token expiry files its coalescing issue on the meta server (`crates/database/tests/it/mcp_tokens.rs`)
- [ ] Seeding leaves the nil server row in place while clearing every other server

## Historical data is retained

- [ ] Status rows written by pingtask before removal (`source = 'canopy'`, non-nil `server_id`) are still present after deploy and still readable on a server's status history
- [ ] Own-status rows against the nil server are still present after deploy
- [ ] A server's point-in-time check reconstruction over a window that includes pingtask-era rows still renders

## Deployment

- [ ] No `pingtask` or `ownstatus` executable is present in the built image
- [ ] The monitor pod runs its sweeps unchanged with the two loops gone, and no pod crashloops on a missing executable
