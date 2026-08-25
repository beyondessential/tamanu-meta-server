# Remove pingtask and decommission related checks

Pingtask is canopy's outbound polling loop: every 60s it HTTP-GETs `/api/public/ping` on every server with a host and inserts a backstop `statuses` row under source `canopy`.
Ownstatus is a sibling loop that logs a status row against the nil-UUID meta server every 60s.
Both go away.

## Scope boundary

The `reachability` check is **not** pingtask and stays entirely.
It is computed by the monitor pod's staleness sweep (`Status::sweep_staleness`) from the freshness of every source a server reports, and pingtask is only one contributor of rows that sweep may read.
Removing pingtask removes a signal *of* reachability, not the concept: the check, its catalog entry, source reachability modes (`on`/`quiet`/`off`), the per-server unreachability silence toggle, and the CHK spec's Reachability section are all untouched.

The nil-UUID server row itself also stays.
It is the sentinel for global-scope issues and the target for MCP token-expiry issues; only the *status logging* against it goes.

Historical data is kept: no migration, no deletion. Existing `statuses` rows with `source = 'canopy'` remain and stay readable.

## Steps

- [x] Delete `crates/jobs/src/bin/pingtask.rs`
- [x] Delete `crates/jobs/src/bin/ownstatus.rs`
- [x] Remove `Status::ping_server`, `Status::ping_servers`, `Status::ping_servers_and_save` from `crates/database/src/statuses.rs`
- [x] Remove `Server::all_pingable` and `Server::own` from `crates/database/src/servers.rs` (both only reachable from the deleted loops)
- [x] Remove `impl Default for NewStatus` — its only purpose was defaulting `source` to `canopy` for ownstatus; public-server constructs every field
- [x] Drop now-unused deps: `reqwest` from the database crate, `hostname` from the jobs crate
- [x] Update comments that describe pingtask as a live path: `monitor.rs` module docs, the sweep's backstop comment, the `Status` field docs for `device_id` / `source`
- [x] `just check`, `cargo fmt`, clippy clean, `just test-package database` + `jobs`

## Notes

`reqwest` was in the database crate solely for the ping client, so the crate no longer makes outbound HTTP at all.
`hostname` was in the jobs crate solely for ownstatus's `extra` payload.

`impl Default for NewStatus` defaulted `source` to `CANOPY_SOURCE`. With canopy no longer generating status rows, a default that attributes a row to canopy is a trap rather than a convenience, so it goes rather than being retargeted.

The reachability sweep's "no counted source" backstop stays: it still covers a server that has never reported, which is not a pingtask concern.

## Out of this repo

The k8s Deployments that run the `pingtask` and `ownstatus` pods live in `beyondessential/ops` (Pulumi), not here.
Deleting the binaries stops them being built into the image; the pod definitions need removing separately or they will crashloop on a missing executable.
