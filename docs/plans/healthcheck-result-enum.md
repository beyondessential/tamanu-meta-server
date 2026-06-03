# Per-check `result` enum (passed / failed / broken / skipped)

## Background

bestool is moving to healthchecks-only (alerts are being retired). Today each
`health[]` entry carries `healthy: bool`, which can't express two states
bestool needs:

- **broken** — the check itself errored or is misconfigured; says nothing
  about the system under test.
- **skipped** — a precondition wasn't met, so the check didn't run.

Rather than bolting side-flags onto the bool (`healthcheckBroken: true`,
`skipped: true` — boolean-blindness, illegal states representable), the wire
format gains a proper sum type:

```json
{ "check": "database", "result": "passed" }
```

`result: "passed" | "failed" | "broken" | "skipped"`.

**Deploy ordering**: canopy ships first. Once every canopy understands
`result`, new bestool emits *only* `result` (no per-check `healthy`, no
flags — those never ship). The flags-interpretation path is therefore not
built at all. The `healthy: bool` fallback stays forever regardless, because
`statuses.health` is stored verbatim and historical rows have it.

Decisions made (with Félix):

- Unrecognised `result` value ⇒ **strict 400** on the push. "Canopy ships
  first" is the standing discipline for future enum additions.
- failed→broken: the open failure issue **stays open**; a **separate**
  broken-check issue is filed at its own ref.
- Top-level `healthy` bool: **unchanged** (absent ⇒ true). Revisit only once
  no bestool sends it. bestool-side changes are out of scope here.

## Semantics

| result  | issue filing                                      | effect on prior open issues |
|---------|---------------------------------------------------|-----------------------------|
| passed  | none                                              | closes `health/<check>` ("recovered") and `health-broken/<check>` |
| failed  | open/keep `health/<check>` at catalog severity (rules engine, as today) | closes `health-broken/<check>` |
| broken  | open/keep `health-broken/<check>` at **fixed Warning** | does **not** close `health/<check>` |
| skipped | none                                              | closes both, failure close message says "skipped" not "recovered" |
| absent  | none (trust the reporter, as today)               | closes both |

- Broken is filed at fixed `Severity::Warning` (visible, below
  `OPENS_INCIDENT`), description `Health check '<check>' is broken`, message
  from `per_check_description` extras. It does not go through the rules
  engine; no per-check override column for now.
- Only **failed** checks reach `HealthcheckSeverity::severity_for`.
- Every well-formed check name seen — any result — still upserts a default
  catalog row.
- `result` joins `check`/`healthy` as a reserved key stripped from
  `check_extra` before rule evaluation and in samples.

## Normalisation

One enum + one normalise-on-read helper, mirrored Rust/TS, used by every
reader (wire *and* stored rows):

- Rust: `CheckResult { Passed, Failed, Broken, Skipped }` in
  `crates/commons-types/src/status.rs`, with
  `CheckResult::from_entry(&serde_json::Map<…>) -> Option<CheckResult>`:
  prefer a valid `result` string, else `healthy: bool` (true→Passed,
  false→Failed), else `None` (malformed entry, ignored as today).
- TS: `CheckResult` union + `checkResultOf(entry)` in `private-web/src`
  (types.ts or a small lib module), used by `parseChecks`.

## Changes

### 1. commons-types — `CheckResult`

- `crates/commons-types/src/status.rs`: enum, `from_entry`, serde +
  `FromStr`/`Display` (wire strings are lowercase), unit tests.

### 2. public-server ingestion (`crates/public-server/src/statuses.rs`)

- `split_health_from_extra` validation per entry: `check` non-empty string
  (unchanged); then **exactly one** of:
  - `result`: string ∈ {passed, failed, broken, skipped} — anything else 400;
  - `healthy`: bool (legacy).
  Both present ⇒ 400; neither ⇒ 400. Error messages name the entry index as
  today. Reuses `AppError::BadRequest`, no new error type (no ERRORS.md
  change).
- Replace `collect_failing_checks` with a collector returning per-result
  sets via `CheckResult::from_entry` (e.g. `BTreeSet` each for failed and
  broken; `collect_all_check_names` keeps feeding the catalog upsert for any
  entry with a valid result).
- `file_health_events`:
  - failed ⇒ open `health/<check>` at catalog severity (as today), stripping
    `check`/`healthy`/`result` from `check_extra`.
  - broken ⇒ open `health-broken/<check>` at fixed Warning,
    `description: "Health check '<check>' is broken"`.
  - closes: prev-failed not in (curr-failed ∪ curr-broken) ⇒ close
    `health/<check>` — message "recovered", or "skipped" when the check is
    currently skipped; prev-broken not in curr-broken ⇒ close
    `health-broken/<check>` ("no longer broken").
  - prev sets come from the stored previous status via the same normaliser,
    so legacy-row → new-push transitions just work.
- `HealthCheck` payload struct (openapi doc shape): `healthy` becomes
  `Option<bool>`, add `result: Option<CheckResult>`, document the
  exactly-one rule and per-state semantics.

### 3. database (`crates/database/src/statuses.rs`)

- `Status::health_state()`: an entry counts toward `Warning` when its
  normalised result is Failed **or** Broken; Passed/Skipped don't.

### 4. private-server

- `fns/statuses.rs` `compute_check_severities`: select Failed entries via
  the normaliser (Broken doesn't need a computed severity — it's fixed
  Warning and the UI renders it from the result directly); strip `result`
  too.
- `fns/healthchecks.rs` `sample`: strip `result` alongside
  `check`/`healthy`.
- `just gen-openapi` after the handler/schema changes; commit
  `private-web/openapi.json` + `private-web/src/api-types.ts`.

### 5. private-web

- `types.ts` (or lib): `CheckResult` union + `checkResultOf` (mirror
  semantics of the Rust helper; document the pairing like
  `healthcheck-rule-eval.ts` does).
- `StatusSnapshot.tsx`:
  - `parseChecks` returns the normalised result; entries with no resolvable
    result are skipped (as today). `result`/`healthy` both excluded from the
    extras listing.
  - sort: failed, broken, skipped, passed; then by name.
  - `CheckIcon` four states: passed → green tick (unchanged); failed →
    severity icon (unchanged); broken → `BuildCircle` (warning colour),
    tooltip "broken — the check itself is failing, not the system";
    skipped → `RemoveCircleOutline` (disabled colour), tooltip "skipped —
    precondition not met".
- `Legends.tsx` health entries are about the server-level `HealthState`
  rollup — wording "some check failing" still holds; no change needed
  unless a per-check icon legend is added later.

### 6. Tests

- commons-types: `CheckResult::from_entry` matrix (result wins, legacy bool,
  malformed).
- public-server `tests/statuses.rs`:
  - validation: valid `result` accepted; unknown string 400; both
    `result`+`healthy` 400; neither 400; legacy `healthy` still accepted.
  - failed files at catalog severity (existing coverage, now via `result`).
  - broken files Warning at `health-broken/<check>`; does not touch an open
    failure issue.
  - failed→broken: failure stays open, broken issue opens; broken→passed
    closes both.
  - skipped: files nothing, closes a prior failure (message mentions
    skipped) and a prior broken issue.
  - legacy prev-row (healthy:false) → new push (result:passed) closes.
- database tests: `health_state` with result-form entries (broken ⇒
  Warning rollup, skipped ⇒ Healthy).
- private-server tests: snapshot `check_severities` covers failed only;
  sample strips `result`.
