# Slack integration

Post canopy incident activity into a Slack channel, and (eventually) let
operators reply in-channel and have their replies land back on the
incident as notes.

## Goal — three tiers

1. **Tier 1 — open notification.** When `Incident::open_for` files a new
   incident, post a message to a single Slack channel.
2. **Tier 2 — threaded activity.** Each tier-1 message anchors a thread.
   Into that thread we post:
   - The opening issue's details (server, source/ref, severity, body).
   - Every subsequent issue join/leave on that incident
     (`Issue::join_incident` / `Issue::leave_incident`, `Issue::resolve`
     / `Issue::reopen` when that flips an incident-member's active flag).
   - Every `IncidentNote::add` against the incident.
   - The incident resolve (`Incident::resolve`) as a final message.
3. **Tier 3 — inbound replies.** Anything a human posts in that thread
   gets ingested as an `IncidentNote` with `author = "slack:<user-id>"`.

## Phase split — what gates on Slack admin

Tier 1 is doable with the Slack Workflow Builder webhook the user can
make themselves. Tiers 2 and 3 require a real Slack app:

| Need                                                  | Webhook | Bot token + signing secret |
| ----------------------------------------------------- | ------- | -------------------------- |
| Post a message in one channel                         | yes     | yes                        |
| Get back a `ts` to anchor a thread                    | **no**  | yes (`chat.postMessage`)   |
| Reply into an existing thread                         | no      | yes                        |
| Receive channel/thread messages (Events API)          | no      | yes                        |
| Verify inbound payloads (signing secret + HMAC)       | n/a     | yes                        |

Workspace admin install is required for the bot token, the `chat:write`
scope, the `message.channels` Events subscription, and pointing Slack at
our public-facing inbound endpoint. **Phase B is gated on that.**

### Design decisions (locked in)

1. **Routing.** Single Slack channel for everything. No per-rank or
   per-server channel selection. (Re-open later if needed.)
2. **Formatting.** Block Kit — header block + section/context fields,
   not plaintext.
3. **Topology.** No new crate for Phase A:
   - Block Kit renderer + outbox model live in `database` (next to the
     enqueue call sites).
   - Worker binary is `crates/jobs/src/bin/slacker_outbox.rs`, alongside
     the existing periodic-task bins.
   - Phase B's inbound webhook *will* want its own bin
     (`crates/jobs/src/bin/slacker_inbound.rs` or a separate crate),
     because it needs to bind a public HTTP listener and isn't a
     periodic loop. Not folded into `public-server` (mTLS-only, device
     auth) or `private-server` (Tailscale-only).
4. **Inbound filtering.** Tier 3 ingests only human messages — drop
   anything with a `bot_id` field, or from canopy's own bot user id.
   Allow-listing other bots is a follow-up, case-by-case.

## Phase A — webhook MVP

### Scope

Two top-level messages per incident, both posted to the configured
webhook URL:

- **Incident opened.** Posted from a hook on `Incident::open_for` when
  the call returns a freshly-created (not reused) incident.
- **Incident resolved.** Posted from a hook on `Incident::resolve`.

That's it. No threads, no per-issue joins, no notes, no inbound.
Webhooks return `200 OK` with the literal body `ok` and no message
metadata, so threading is physically impossible on this path.

### Wiring

- Workflow Builder webhooks bind 1:1 to a workflow with a fixed set of
  declared trigger variables. Block Kit lives in the workflow editor;
  canopy POSTs flat JSON keyed by those variable names. Two workflows
  (open + resolve) with two webhook URLs.
- Variable payload renderer at `crates/database/src/slack_outbox/vars.rs`,
  alongside the model.
- New table `slack_outbox`:
  ```
  id            UUID PK
  created_at    TIMESTAMPTZ DEFAULT NOW()
  kind          TEXT     -- 'incident_open' | 'incident_resolve'
                         -- (Phase B adds: 'issue_join' | 'issue_leave'
                         --                'incident_note')
  incident_id   UUID REFERENCES incidents(id)
  issue_id      UUID NULL REFERENCES issues(id)    -- Phase B
  note_id       UUID NULL REFERENCES incident_notes(id)  -- Phase B
  payload       JSONB    -- Flat workflow-variables object rendered at enqueue time
  delivered_at  TIMESTAMPTZ NULL
  attempts      INT NOT NULL DEFAULT 0
  last_error    TEXT NULL
  ```
  The outbox pattern decouples "the state change committed" from "Slack
  is reachable right now", and the worker is the only piece that talks
  to Slack — so we have one place to put rate-limit / retry logic.
- Enqueue points in `crates/database/`:
  - `Incident::open_for`: after the insert, when the row is new, also
    insert a `slack_outbox` row with `kind = 'incident_open'`.
    Render-at-enqueue means we capture severity/server/source-ref as
    they were at open time, not at delivery time.
  - `Incident::resolve`: same pattern, `kind = 'incident_resolve'`.
  - Both happen inside the existing transaction so an outbox row never
    exists without its incident.
- New binary `crates/jobs/src/bin/slacker_outbox.rs` modelled on
  `reachability.rs`:
  - Tick every 5 seconds (cheap — only does work when rows exist).
  - `SELECT ... FROM slack_outbox WHERE delivered_at IS NULL ORDER BY
    created_at LIMIT 10 FOR UPDATE SKIP LOCKED`.
  - POST the row's payload verbatim to the URL matching `row.kind`
    (`SLACK_WEBHOOK_OPEN_URL` for `incident_open`,
    `SLACK_WEBHOOK_RESOLVE_URL` for `incident_resolve`). No wrapper —
    Workflow Builder consumes the top-level object as trigger variables.
  - On 200: `UPDATE slack_outbox SET delivered_at = NOW()`.
  - On error: `attempts = attempts + 1, last_error = ?`. Give up after
    N attempts (log + leave delivered_at NULL with a final last_error
    note — operator can clear by hand). N defaults to 10.
- Wire `slacker_outbox` into whatever supervisor runs `reachability`,
  `pingtask`, etc. so deployment is the same shape.

### Workflow variables (Phase A)

Slack-side: two workflows in Workflow Builder, each with a webhook
trigger declaring these variables (all text). The Block Kit message
itself is composed in the workflow editor, referencing the variables.

Open (`SLACK_WEBHOOK_OPEN_URL`):
- `server` — `<name> (<host>)` or just `<host>`
- `severity` — `Error`, `Critical`, …
- `source_ref` — `canopy/reachability`
- `message` — issue body
- `link` — canopy incident URL

Resolve (`SLACK_WEBHOOK_RESOLVE_URL`):
- `server` — same shape as open
- `by` — operator name/email or `automation` for cascade-close
- `link` — canopy incident URL

The canopy incident link points at the private-server (admin UI,
Tailscale-gated) at `<PRIVATE_URL>/incidents/<id>`. Operators receiving
Slack notifications are on Tailscale, so `PRIVATE_URL` is the right
base — `PUBLIC_URL` is the device-facing API origin and would render
a 404 / unauthorised page for an operator who clicked through. If
`PRIVATE_URL` is unset, `link` falls back to `http://localhost/...`
so the workflow's `<{{link}}|Open in canopy>` mrkdwn still renders as
a clickable (broken) link rather than malformed text. Set
`PRIVATE_URL` in any env that posts to a real Slack.

### Configuration

- `SLACK_WEBHOOK_OPEN_URL` — Workflow Builder webhook for the
  `incident_open` workflow. Absent → opens are dropped (marked
  delivered, never posted).
- `SLACK_WEBHOOK_RESOLVE_URL` — same, for the `incident_resolve`
  workflow.
- `PRIVATE_URL` — base URL of the private-server admin UI (e.g.
  `https://canopy.example.ts.net`). Gates the `link` field.

### Out of scope for Phase A

- Threads (impossible without a `ts`).
- Per-issue or per-note posts (would clutter the channel as top-level
  messages with no incident grouping).
- Inbound (no signing secret, no Events subscription).

## Phase B — bot token + threads + inbound

Phase A keeps working unchanged when the bot token is absent. When
`SLACK_BOT_TOKEN` is present, the outbox worker switches to
`chat.postMessage` and the inbound binary becomes useful.

### Admin asks (collect these from a Slack workspace admin)

- Create a Slack app installed into the workspace.
- Scopes: `chat:write`, `chat:write.public` (so we can post into a
  channel we're not a member of), `channels:history` (Events API).
- Bot user OAuth token: `xoxb-…` → `SLACK_BOT_TOKEN`.
- Signing secret → `SLACK_SIGNING_SECRET`.
- Enable Events API, subscribe bot to `message.channels`, point the
  Request URL at `https://<canopy-public-host>/slack/events`.
- The channel id (`C…`) we're posting into → `SLACK_CHANNEL_ID`.
- Invite the bot user to the channel (or rely on
  `chat:write.public`).

### Schema additions

```sql
CREATE TABLE slack_threads (
  incident_id  UUID PRIMARY KEY REFERENCES incidents(id),
  channel      TEXT NOT NULL,
  thread_ts    TEXT NOT NULL,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX slack_threads_channel_ts ON slack_threads (channel, thread_ts);
```

The (channel, ts) lookup is what the inbound endpoint hits when a reply
lands — that's why it gets its own index even though the table is
keyed by incident.

`slack_outbox` gains `kind` variants:

- `incident_open` — same as Phase A but posted via `chat.postMessage`;
  on success, insert into `slack_threads`.
- `incident_resolve` — posted as a reply into the thread.
- `issue_join` — issue joined the incident.
- `issue_leave` — issue left (resolved or no longer matches).
- `incident_note` — operator added a note in canopy.

Enqueue points in `crates/database/`:

- `Incident::open_for` (new) — Phase A path, schema unchanged.
- `Incident::resolve` — Phase A path.
- `Issue::join_incident` — enqueue `issue_join` with `issue_id`.
- `Issue::leave_incident` — enqueue `issue_leave`.
- `Issue::resolve` / `Issue::reopen` — when these end up calling
  `join`/`leave` they go through the same hook; no separate enqueue
  needed.
- `IncidentNote::add` — enqueue `incident_note` with `note_id`, **but
  only if `author != "slack:..."`**. That's the echo-prevention
  trapdoor: notes that came from Slack don't get reposted to Slack.

### Outbox worker — Phase B branch

When `SLACK_BOT_TOKEN` is set:

- For `incident_open`: POST `chat.postMessage` to
  `SLACK_CHANNEL_ID`, capture `ts` from response, upsert into
  `slack_threads`.
- For everything else: look up `slack_threads.thread_ts` by
  `incident_id`, POST `chat.postMessage` with `thread_ts` set.
- If `slack_threads` has no row (open was never posted, e.g. because
  the channel was misconfigured), drop the follow-up with a logged
  warning rather than spamming the channel as a top-level message.

Rate limits: `chat.postMessage` is Tier-3 (~50/min/workspace). The
worker batches with `LIMIT 10` and a 5-second tick, which is well under
that — no token bucket needed unless we see 429s in practice.

### Inbound endpoint

New binary `crates/jobs/src/bin/slacker_inbound.rs` running an axum
server, single route:

```
POST /slack/events
```

Steps per request:

1. **Verify signature.** Read `X-Slack-Request-Timestamp` and
   `X-Slack-Signature`, reject if timestamp is older than 5 minutes,
   recompute HMAC-SHA256 with `SLACK_SIGNING_SECRET`, constant-time
   compare.
2. **URL verification.** If `type == "url_verification"`, echo back
   `challenge`. This is how Slack proves it can reach us.
3. **Event callback.** For `event.type == "message"`:
   - Drop if `subtype` is set (edits, deletes, joins — handle later
     if useful).
   - Drop if `bot_id` is present, or if `user` equals our own bot
     user id (config: `SLACK_OWN_BOT_USER_ID`, set once after install).
     This is the canopy-doesn't-import-itself guarantee.
   - Drop if `thread_ts` is missing (top-level reply unrelated to an
     incident).
   - Look up `slack_threads` by `(channel, thread_ts)`. If miss, drop
     silently — the thread isn't one of ours.
   - Insert `IncidentNote { incident_id, author = "slack:<user>",
     body = text }`. The Phase B `IncidentNote::add` enqueue hook
     above skips re-posting because of the `author` prefix.
4. Always return `200 OK` quickly (Slack retries on non-2xx and on
   timeouts > 3s).

Hosting note: this endpoint has to be reachable from Slack's egress,
which means going through the same edge that `public-server` uses, but
with **no mTLS** — Slack authenticates with the signing secret in a
header. Easiest is its own subdomain/port on the same reverse proxy,
mTLS off. Don't try to share the public-server binary; the auth model
is fundamentally different and the failure mode of getting that wrong
is "Slack can't reach us" or worse "anyone with the URL can post
notes".

### Echo prevention — restated

Two independent barriers, on purpose:

1. **Outbox enqueue side.** `IncidentNote::add` only enqueues a
   `slack_outbox` row when `author` doesn't start with `slack:`.
2. **Inbound side.** Drop messages with a `bot_id`, or from our own
   bot user id.

Either barrier alone would in principle work, but the inbound side
also keeps third-party bot chatter out, and the outbox side handles
the case where someone backdoors a `slack:`-authored note via the UI
(should never happen, but cheap to guard).

### Block Kit payloads (Phase B summary)

- `incident_open` — same as Phase A but routed via bot.
- `issue_join` — `🟧 Issue joined incident — <source>/<ref>
  · <severity>` plus section with body.
- `issue_leave` — `🟩 Issue resolved` with who/when.
- `incident_note` — header `📝 <author>`, section with body.
- `incident_resolve` — `✅ Incident resolved by <who>`.

All five share a small renderer module in `crates/slacker/blocks.rs` so
the Phase A and Phase B variants stay visually consistent.

## Phasing / unplan criteria

This plan is "done" (and gets unplanned) when:

- Phase A is shipped: `SLACK_WEBHOOK_URL` set → opens and resolves
  appear in the channel. `SLACK_WEBHOOK_URL` unset → no observable
  difference vs. today.
- Phase B is shipped: `SLACK_BOT_TOKEN` + `SLACK_SIGNING_SECRET` set
  → threads under each incident message contain joins/leaves/notes,
  and Slack-side replies appear as `IncidentNote`s in canopy. With
  both unset, Phase A's behaviour is unchanged.
- An end-to-end test exists for Phase A (mock Slack endpoint, assert
  outbox drains and POST body matches expected blocks).
- An end-to-end test exists for the Phase B inbound path (signed
  request → note inserted; unsigned → rejected; bot-id present →
  dropped).

If Phase B stalls waiting for the workspace admin, ship Phase A
standalone, leave this plan in place, and unplan only after Phase B
lands.

## Open follow-ups (not blocking)

- Per-rank or per-tag channel routing.
- Allow-list specific third-party bots through tier-3 ingestion.
- Slack-side acks (`/canopy ack <incident>`) — would need a slash
  command + interactivity URL, both already covered by the same app
  install if we want them later.
- Backfill: posting a notice when canopy starts up if there are open
  incidents with no Slack thread. Probably not worth it; old incidents
  would re-spam.
