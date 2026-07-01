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

## Open follow-ups (not blocking)

- Per-rank or per-tag channel routing.
- Allow-list specific third-party bots through inbound ingestion.
- Slack-side acks (`/canopy ack <incident>`) — needs a slash command +
  interactivity URL, both covered by the same app install.
