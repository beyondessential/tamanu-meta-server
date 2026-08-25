---
status: draft
---

# Split the core model: machines, application servers, and identities

Separate Canopy's `server` into a machine and an application server, and retire `device` into an identity and a machine, so machine-level facts have somewhere of their own to live.

## Terminology

Direction agreed: rename rather than qualify.
"Server" means too many things, and "application server" carries the confusion forward.

- **Application** — what a `server` is today, minus its machine-ish columns. What the status page presents.
- **Machine** — the host an application runs on. New concept, though bestool's existing server ID is really this.
- **Identity** — what a `device` is today: a set of keys, with an optional Tailscale identity. Not a machine.

A machine usually has an identity, and one or more applications on it.

## Behaviour

### Cardinality

- An application has zero or one machine, never two.
- A machine has one or more applications.
  Whether "one or more" is enforced is an open question (see below) — the delete-then-recreate case wants a temporary zero.
- On Kubernetes an application has no machine at all; machine-less is a first-class case, not a degenerate one.
- A Kubernetes cluster is a separate entity beside machines, not a machine of a special class.

### How a machine comes into being

There is no standalone "create a machine" flow, at least not initially.
Machines are created through creating applications, which keeps the invariant that a machine has applications on it reachable by construction.

- The migration creates one machine per existing server, 1:1.
- The application creation form gains a machine section.
  By default it creates a new machine for that application.
  Unticking that offers a search dropdown to select an existing machine instead.

### Monitoring and reachability

Working position: reachability is a machine-level concept only, and applications have nothing called reachability.
Applications carry a monitoring on/off toggle; machines carry one too.
See the reachability findings below — pingtask and the legacy `tamanu` heartbeat complicate this and it is not settled.

## Reachability as it stands today

Three distinct things wear the name, and they do not split the same way.

**1. `ShortStatus`** (`crates/commons-types/src/status.rs:49`) — up / blip / away / down / gone, derived purely from the age of the latest status row for a server against fixed thresholds (2, 10, 30 minutes).
Presentational only: status dots, the MCP fleet summary's `unreachable` count.
Nothing alerts off it.

**2. The `canopy`/`reachability` check** (`Status::sweep_staleness`, `crates/database/src/statuses.rs:275`) — one check per server, computed from per-source freshness against that server's own `alert_when_down_for` interval, each source graded by its reachability mode (`on`/`quiet`/`off`).
This is the one that alerts, is silenceable (the "Alert when this server is unreachable" switch in `ServerEdit.tsx` *is* the server-scoped silence on it), and feeds incidents and the health rollup.

**3. Pingtask** (`Status::ping_server`, `crates/database/src/statuses.rs:175`) — an active HTTP GET of the server's `host` + `/api/public/ping`, run only for servers with **no device** (`all_pingable` filters on `device_id IS NULL`).
It writes a synthetic status row under the `canopy` source, so it feeds 1 and 2 rather than standing on its own.

### What that implies for the split

1 and 2 are both "did a reporter reach us recently".
The reporter is bestool, and bestool runs on the machine, so both are machine facts.
That supports machine-primary.

3 is not.
An HTTP GET against the application's own endpoint asks whether the *application* is serving, which a machine grain cannot answer: a live box running a dead Tamanu keeps bestool reporting while the ping fails.
It only looks like a machine fact today because it is laundered through a synthetic status push.

The legacy `tamanu` source (see [STA](../../specs/public-server/statuses.md), "Legacy pushes") is the same shape.
A Tamanu server pushing its own heartbeat is an application reporter, so its silence is an application fact, and today that silence is exactly what moves the server's reachability check.

So the real discriminator is not the target but **what a source reports about** — the same subject axis L2 is giving bestool.
Reachability is "did the reporters that file against this target go quiet", and which target that is follows from the source's subject.

Health is already documented as independent of reachability (`status.rs:406`), so the two axes are established; this adds a grain to one of them.

## Open questions

- [ ] Does an application keep a reachability signal of its own for application-subject reporters (pingtask, the legacy `tamanu` heartbeat), or does application liveness stop being called reachability and become an ordinary check?
- [ ] If applications do keep one, is it suppressed while the machine is unreachable, so a dead host is one fact rather than N?
- [ ] Is "a machine has at least one application" enforced, or is a temporary zero allowed for the delete-then-recreate case?
- [ ] Does an identity gain a machine association, or is the machine link a property of what reports rather than of who authenticates?
- [ ] Where do `host`, `cloud`, and `geolocation` land, and what does that do to DNS, TLS, and backups, which reach for them today?
- [ ] What does the status page present once applications and machines are separate?

## Transition

bestool's existing server ID is really the machine ID.

- bestool keeps calling it the server ID for now, and Canopy keeps accepting it as such.
- Canopy introduces a machine ID; bestool moves across to it later.
- bestool will grow separate application-subject and machine-subject reporting.
- Canopy needs to recognise a bestool that is not yet sending split data, and split the current coalesced push across the two grains itself.
