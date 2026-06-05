# Operator presence from the `external_users` healthcheck

The `external_users` doctor check (bestool alertd) reports interactive login
sessions on each status push. Each entry in `health[]` looks like:

```json
{
  "check": "external_users",
  "result": "passed" | "warning" | "skipped",
  "count": 2,
  "users": [
    {
      "name": "besd",
      "line": "rdp-tcp#0",
      "login": "2026-06-05T01:00:00Z",
      "connected_since": "2026-06-05T01:05:00Z",
      "source": "100.x.y.z",          // optional
      "tailscale": "alice@example.com", // optional: Tailscale login via whois
      "session_id": 2                   // optional, Windows only
    }
  ]
}
```

Sessions with a `tailscale` login are *operators* — identified humans connected
over Tailscale. We surface them in two places:

1. **Server detail page**: promote the check out of the generic checks table
   into a headline strip — "N operators in the server right now" with avatars
   (Tailscale profile pic, falling back to a first-letter-of-email avatar) —
   and format the check row's `users` blob as readable session rows instead of
   raw JSON.
2. **Status page**: group cards whose members have active operator presence get
   a subtly shaded background and a person icon-chip alongside the incident
   chip, with a tooltip naming who's on which server.

## Semantics

- **Operator** = distinct `tailscale` login among the check's `users[]`. One
  person with several sessions counts once; their `connected_since` is the
  earliest across their sessions.
- Sessions *without* a Tailscale identity (local console, non-Tailscale SSH)
  are not operators. They still appear in the formatted session rows on the
  detail page ("other sessions"), but don't drive the headline count or the
  status-page marking.
- **Freshness**: presence is read from the latest status push. The status page
  only marks groups when the member is actively reporting (`up` ∈ {up, blip}) —
  a stale push can't claim "someone is in the server right now". The detail
  page headline gets the same gate (the checks table still shows the session
  rows from the last push regardless, consistent with how stale checks display
  today).

## Backend

### commons-types (`crates/commons-types/src/status.rs`)

- New wire type:
  ```rust
  pub struct OperatorPresence {
      pub login: String,                    // tailscale login (email)
      pub name: Option<String>,             // from tailscale_users cache
      pub profile_pic: Option<String>,      // from tailscale_users cache
      pub connected_since: Option<Timestamp>, // earliest across sessions
  }
  ```
- New parser `operators_from_health(health: &serde_json::Value) -> Vec<OperatorPresence>`:
  finds the `external_users` entry, collects distinct `tailscale` logins with
  earliest `connected_since`, leaves `name`/`profile_pic` as `None` (enrichment
  is the private-server's job). Lenient: malformed entries are skipped.
- Unit tests: dedupe, earliest-since, missing `tailscale`, malformed shapes.

### database (`crates/database/src/statuses.rs`)

- Thin `Status::operators(&self) -> Vec<OperatorPresence>` delegating to the
  commons-types parser.

### private-server

- Shared enrichment helper (e.g. in `fns/statuses.rs`): given
  `Vec<OperatorPresence>`, batch `CachedTailscaleUser::by_logins` and fill
  `name`/`profile_pic`. Mirrors the issues/incidents `lookup_user` pattern.
- `FacilityServerStatus` (`crates/commons-types/src/server/cards.rs`) gains
  `operators: Vec<OperatorPresence>`. `group_details` populates it from the
  already-fetched latest statuses, gated server-side on the member's
  `short_status()` being up/blip (the field means *active* presence), with one
  `by_logins` batch across all members.
- `ServerLastStatusData` (`fns/servers.rs`) and `StatusSnapshotData`
  (`fns/statuses.rs`) gain `operators: Vec<OperatorPresence>`, enriched, not
  gated (the detail page knows `up`; the snapshot is explicitly historical).
- `just gen-openapi`; commit `openapi.json` + `api-types.ts` alongside.

### Tests

- private-server: `group_details` members carry enriched operators (cached
  user → name/pic present; unknown login → bare); gating when the member isn't
  reporting; `last_status`/`snapshot` carry operators.

## Frontend (`private-web/`)

### New components

- `components/OperatorAvatars.tsx`: `OperatorPresence[]` → MUI `AvatarGroup`
  of small avatars. Each avatar: `src=profile_pic`, fallback content = first
  letter of `login` uppercased; tooltip `name (login) — connected 3h 12m`
  (duration from `connected_since`, reusing the existing humanize helper).
- `components/ExternalUsersDetails.tsx` (shared by ServerDetail's checks table
  and StatusSnapshot's checks block): formats the check's `users[]` as session
  rows — identity (Tailscale login or OS username), line/source, connected
  duration — joining avatar display info from `operators` by login. Falls back
  to the generic key/value rendering if the shape is unexpected.

### Server detail (`routes/ServerDetail.tsx`)

- `InfoSection`: when reporting and `operators.length > 0`, render a headline
  strip near the health chip: avatars + "N operator(s) in the server right
  now".
- `ChecksTable`/`CheckRow`: special-case `check === "external_users"` to render
  `ExternalUsersDetails` instead of the raw `users` JSON in the extras dl
  (other extras like `count` are subsumed by the formatted view).

### Status snapshot (`components/StatusSnapshot.tsx`)

- `ChecksBlock`: same special-case via `ExternalUsersDetails`, with "as of"
  wording (no "right now" claim for historical snapshots).

### Status page (`routes/Status.tsx`)

- `GroupCard`/`GroupCardLoader`: members with non-empty `operators` →
  - card background gets a subtle tint (composing with the existing hover
    style and incident border),
  - an icon chip (person icon + operator count) renders alongside the incident
    chip, tooltip listing `login · server name` lines.
- `components/Legends.tsx`: add an entry explaining the shading/icon.

### Checks

- `just typecheck` after frontend changes.
