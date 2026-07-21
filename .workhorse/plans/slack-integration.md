# Slack integration — threads + inbound (unshipped)

Incident open/resolve notifications already post to a Slack channel via the
Workflow Builder webhook path (the `slack_outbox` table, its `incident_open`
/ `incident_resolve` kinds, and the `slacker_outbox` worker). This plan
covers the remaining work: upgrading from single top-level messages to
threaded per-incident activity, and ingesting operator replies back onto the
incident.

## Goal

With a real Slack app (bot token + signing secret) present:

- **Threaded activity.** The incident-open message anchors a thread; every
  subsequent issue join/leave, note, and the final resolve posts as a reply
  in that thread instead of a standalone channel message.
- **Inbound replies.** Anything a human posts in that thread is ingested as
  an `IncidentNote` with `author = "slack:<user-id>"`.

The existing webhook path keeps working unchanged when the bot token is
absent; when `SLACK_BOT_TOKEN` is present the outbox worker switches to
`chat.postMessage` and the inbound listener becomes useful.

## Decisions (locked in)

- **Single channel** for everything — no per-rank/per-server routing.
- **Block Kit** formatting (header + section/context), not plaintext.
- **Inbound filtering:** ingest only human messages — drop anything with a
  `bot_id`, or from canopy's own bot user id.
- **Topology:** the outbox model stays in `database` and the drain worker in
  `crates/jobs/src/bin/slacker_outbox.rs`. The inbound webhook gets its own
  binary (it binds a public HTTP listener and isn't a periodic loop); it is
  not folded into `public-server` (mTLS-only) or `private-server`
  (Tailscale-only).

## Admin asks (from a Slack workspace admin)

- A Slack app installed into the workspace.
- Scopes: `chat:write`, `chat:write.public` (post to a channel we're not a
  member of), `channels:history` (Events API).
- Bot user OAuth token `xoxb-…` → `SLACK_BOT_TOKEN`; signing secret →
  `SLACK_SIGNING_SECRET`; channel id `C…` → `SLACK_CHANNEL_ID`.
- Events API enabled, bot subscribed to `message.channels`, Request URL
  pointed at `https://<canopy-public-host>/slack/events`.

## Schema additions

```sql
CREATE TABLE slack_threads (
  incident_id  UUID PRIMARY KEY REFERENCES incidents(id),
  channel      TEXT NOT NULL,
  thread_ts    TEXT NOT NULL,
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX slack_threads_channel_ts ON slack_threads (channel, thread_ts);
```

The `(channel, thread_ts)` lookup is what the inbound endpoint hits when a
reply lands — hence its own index even though the table is keyed by incident.

`slack_outbox` gains kinds: `issue_join`, `issue_leave`, `incident_note`
(alongside the existing `incident_open` / `incident_resolve`), plus the
nullable `issue_id` / `note_id` references they carry.

## Enqueue points (`crates/database/`)

- `Issue::join_incident` → `issue_join` (with `issue_id`).
- `Issue::leave_incident` → `issue_leave`. `Issue::resolve` / `Issue::reopen`
  go through the same join/leave hooks; no separate enqueue.
- `IncidentNote::add` → `incident_note` (with `note_id`), **only if `author`
  doesn't start with `slack:`** — the echo-prevention trapdoor so notes that
  came from Slack aren't reposted.

## Outbox worker — bot branch

When `SLACK_BOT_TOKEN` is set:

- `incident_open`: POST `chat.postMessage` to `SLACK_CHANNEL_ID`, capture
  `ts`, upsert `slack_threads`.
- Everything else: look up `slack_threads.thread_ts` by `incident_id`, POST
  `chat.postMessage` with `thread_ts` set.
- If `slack_threads` has no row (open never posted), drop the follow-up with
  a logged warning rather than posting a stray top-level message.

`chat.postMessage` is Tier-3 (~50/min/workspace); the existing `LIMIT 10` /
5-second tick is well under that — no token bucket unless 429s show up.

## Inbound endpoint

New binary (axum server, single `POST /slack/events` route). Per request:

1. **Verify signature** — reject if `X-Slack-Request-Timestamp` is older than
   5 minutes; recompute HMAC-SHA256 with `SLACK_SIGNING_SECRET`, constant-time
   compare against `X-Slack-Signature`.
2. **URL verification** — if `type == "url_verification"`, echo `challenge`.
3. **Event callback** — for `event.type == "message"`:
   - Drop if `subtype` is set (edits/deletes/joins).
   - Drop if `bot_id` is present or `user` equals our own bot id
     (`SLACK_OWN_BOT_USER_ID`).
   - Drop if `thread_ts` is missing.
   - Look up `slack_threads` by `(channel, thread_ts)`; miss → drop silently.
   - Insert `IncidentNote { incident_id, author = "slack:<user>", body = text }`.
4. Always return `200 OK` within ~3s (Slack retries on non-2xx / timeouts).

Hosting: reachable from Slack's egress, so it goes through the same edge as
`public-server` but with **no mTLS** — Slack authenticates via the signing
secret. Give it its own subdomain/port on the reverse proxy; do not share the
public-server binary (the auth model differs, and getting it wrong means
either "Slack can't reach us" or "anyone with the URL can post notes").

## Echo prevention — two independent barriers

1. **Enqueue side:** `IncidentNote::add` only enqueues when `author` doesn't
   start with `slack:`.
2. **Inbound side:** drop messages with a `bot_id` or from our own bot id.

Either alone would work; the inbound side also keeps third-party bot chatter
out, and the enqueue side guards against a `slack:`-authored note reaching the
UI by some other path.

## Block Kit payloads

- `issue_join` — `🟧 Issue joined incident — <source>/<ref> · <severity>` + body.
- `issue_leave` — `🟩 Issue resolved` with who/when.
- `incident_note` — header `📝 <author>` + body.
- `incident_resolve` — `✅ Incident resolved by <who>` (posted into the thread).

## Done when

- With `SLACK_BOT_TOKEN` + `SLACK_SIGNING_SECRET` set: each incident message
  anchors a thread carrying joins/leaves/notes, and Slack-side replies appear
  as `IncidentNote`s. With both unset, the webhook path is unchanged.
- An end-to-end test for the inbound path: signed request → note inserted;
  unsigned → rejected; `bot_id` present → dropped.

## Querying canopy from Slack (draft — exploratory)

Once the bot-token app is installed, the same install can carry an inbound
**query** path: a person (or an in-Slack agent) invokes a canopy query in a
channel, canopy runs it and replies in-thread. The query runs against the
**same read-only tool layer the MCP exposes** (`crates/private-server/src/mcp.rs`,
spec `MCP`) — we do not build a second set of fleet queries.

There is **no LLM in this path.** The Slack side is dumb: it names an MCP tool
and its arguments, canopy dispatches to that tool, and the reply is the tool's
JSON plus a deterministic, machine-generated summary. Interpreting free-text
questions is out of scope — an agent that wants that runs its own model against
`/api/mcp` directly. Slack just relays.

### What's being reused

`CanopyMcp` is a router of read-only tools (`find_servers`, `get_server`,
`find_incidents`, `fleet_summary`, `find_backup_problems`, …) that shape lean
JSON straight off the `database` read functions. A caller in Slack picks one of
those tools and its args exactly as an MCP client would; canopy runs it and
returns the result. Read-only means the worst a bad or malformed query can do is
produce an unhelpful reply — nothing mutates the fleet.

Dispatch is **in-process**: the query worker holds an `AppState` and drives the
same tool router (or, more cleanly, the tool bodies are factored out of the
`#[tool]` methods so both the MCP mount and this path call the same functions).
No second query implementation, no network hop, and it works even though the
MCP mount itself is Tailscale-only.

### Invocation syntax

The message carries a tool name plus arguments — the MCP call, spelled for a
chat box. Two encodings, both parsed deterministically:

- **Agent / precise:** a JSON object, e.g.
  `@canopy {"tool": "find_incidents", "args": {"since_days": 7, "status": "open"}}`.
- **Human shorthand:** `@canopy find_incidents since_days=7 status=open` —
  `key=value` pairs coerced against the tool's argument schema.

`@canopy help` (or `tools`) lists the available tools and their arguments,
generated from the router's schemas — so discovery is also code, not docs that
drift. Unknown tool or bad args → a deterministic error reply naming the
offending field, mirroring the MCP `invalid_params` messages.

App mention is the entry point, reusing the Events API path this plan already
stands up: the inbound binary gains an `event.type == "app_mention"` branch. A
mention outside a thread opens a thread for the answer; inside a thread it
answers in place. (A slash command `/canopy …` is a possible addition but needs
its own Request URL + `response_url` ack dance — deferred.)

### Flow

A single tool dispatch is one-to-a-few DB reads and normally well inside Slack's
3-second Events API budget, but posting the reply (and any file upload) is a
separate round-trip to Slack. To keep the ack path trivial and get durable
retry on the Slack write, reuse the outbox-style queue:

1. Inbound endpoint validates the signature (existing code), recognises an
   `app_mention`, parses out `{tool, args}`, inserts a `slack_query` row,
   returns `200 OK`.
2. A worker (`crates/jobs/src/bin/slacker_query.rs`, mirroring `slacker_outbox`)
   claims the row, dispatches the tool, and posts the reply — directly via
   `chat.postMessage` + `files_upload_v2`, or by enqueuing an outbox row so all
   Slack writes stay on one path.

Parse/dispatch failures post a deterministic error into the thread rather than
dropping silently.

### The reply — deterministic summary + JSON attachment

Tool JSON (a `find_servers` over the fleet, a `get_group` with backup history)
easily blows past what's readable in a Slack message, and Slack caps section
text around 3k characters. So the reply is **two-tier**:

- **In the message:** a compact, machine-generated summary — *not* the raw JSON.
  Each result type gets a small `summarize()` formatter (counts + the few fields
  that matter), the same spirit as the Block Kit payloads above but for query
  results. Examples:
  - `find_servers` → `42 matched, 38 shown · 3 unhealthy, 5 unreachable`.
  - `find_backup_problems` → `7 problems: 2 overdue, 1 provisioning error, 4 failed runs`.
  - `fleet_summary` → the rollup counts inline.
  These formatters are plain Rust over the typed result structs — no model.
- **As an attachment:** the full tool result uploaded as a file **snippet** via
  `files_upload_v2` into the same thread (`thread_ts` set), so agents get the
  complete structured payload and humans can expand it. Format, to pick:
  - `.json` — the raw tool result; the default, and what an agent consumes.
  - `.csv` / `.md` table — friendlier for eyeballing server/incident lists;
    could be offered for the list-shaped results.

  Slack renders snippets inline with expand/collapse, so a big result doesn't
  wall-of-text the channel but is one click (or one download) away. Small
  results (under the section limit) can skip the attachment and inline the JSON
  in a code block instead — threshold TBD.
- **Deep links where one exists.** The summary can link to `PRIVATE_URL/...`
  for a named server/group/incident so an operator lands on the page where they
  can act. (`PRIVATE_URL` is the admin UI; see the URL-split rule.)

### Auth & attribution

The MCP mount gates on *any authenticated tailnet user* and logs the login.
Over Slack there is no tailnet identity — the trust boundary is **workspace
(and channel) membership**. Decisions to lock in:

- Restrict the query path to a specific channel (or channel allow-list) — an
  ops channel — rather than answering anywhere the bot is mentioned. The data
  is read-only but not something to expose to a general workspace.
- Attribute every query as `slack:<user-id>` in logs, matching the inbound-note
  author convention, so who-asked-what stays auditable.
- Reuse the existing `bot_id` / own-bot-id drop so the bot never answers itself
  or loops on another bot's chatter.

### Admin asks (additions to the list above)

- `app_mentions:read` scope and the `app_mention` event subscription.
- `files:write` scope (snippet uploads).

### New config

- `SLACK_QUERY_CHANNEL_ID` (or reuse `SLACK_CHANNEL_ID`) for the allow-list.
- No API key or model config — there is no LLM in this path.

### Schema additions (draft)

```sql
CREATE TABLE slack_query (
  id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  channel      TEXT NOT NULL,
  thread_ts    TEXT NOT NULL,
  slack_user   TEXT NOT NULL,
  tool         TEXT NOT NULL,   -- the MCP tool name invoked
  args         JSONB NOT NULL,  -- parsed arguments
  status       TEXT NOT NULL DEFAULT 'pending', -- pending | done | failed
  answer_ts    TEXT,            -- ts of the posted reply
  created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  answered_at  TIMESTAMPTZ
);
```

Mirrors `slack_outbox`'s claim/drain shape so the worker is a near-copy.

### Open questions

- **Dispatch reuse.** Confirm rmcp exposes a clean by-name invoke over the tool
  router, or factor the tool bodies out of the `#[tool]` methods so the MCP
  mount and this path share one set of functions (preferred — avoids depending
  on rmcp internals).
- **Summary coverage.** Which tools get a bespoke `summarize()` vs. a generic
  "N records, see attachment" fallback.
- **Inline vs. attachment threshold.** Byte/row cutoff for inlining JSON in a
  code block instead of uploading a snippet.
- **Rate guard.** Per-user or per-channel throttle to bound abuse (cheap, but a
  fleet-wide `find_servers` in a loop is still noise).

## Open follow-ups (not blocking)

- Per-rank or per-tag channel routing.
- Allow-list specific third-party bots through inbound ingestion.
- Slack-side acks (`/canopy ack <incident>`) — needs a slash command +
  interactivity URL, both covered by the same app install.
