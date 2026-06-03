# Per-check `result` enum (passed / warning / failed / broken / skipped)

## Background

bestool is moving to healthchecks-only (alerts are being retired). Today each
`health[]` entry carries `healthy: bool`, which can't express two states
bestool needs:

- **broken** — the check itself errored or is misconfigured; says nothing
  about the system under test.
- **skipped** — a precondition wasn't met, so the check didn't run.

and a third it already sends but canopy can't see: **warning** — the check
ran, the system is degraded but not failing. Legacy bestool encodes that as
per-check `healthy: false` with top-level `healthy: true`, which canopy
ignores (it does its own severity mapping and doesn't consult top-level).

Rather than bolting side-flags onto the bool (`healthcheckBroken: true`,
`skipped: true` — boolean-blindness, illegal states representable), the wire
format gains a proper sum type:

```json
{ "check": "database", "result": "passed" }
```

`result: "passed" | "warning" | "failed" | "broken" | "skipped"`.

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
- warning default severity: **fixed `Severity::Warning`** when no custom
  rule matches; the catalog's severity column applies to **failed** only.
- Catalog default severity for failed stays **Warning** (no change).
- Legacy warning encoding (per-check false + top-level true) is **not**
  disambiguated: legacy `healthy: false` maps to failed, exactly today's
  behaviour — no regression during the fleet transition, and top-level is
  too ambiguous with mixed pushes.

## Semantics

| result  | issue filing                                      | effect on prior open issues |
|---------|---------------------------------------------------|-----------------------------|
| passed  | none                                              | closes `health/<check>` ("recovered") and `health-broken/<check>` |
| warning | open/keep `health/<check>`: custom rules first, else fixed Warning | closes `health-broken/<check>` |
| failed  | open/keep `health/<check>`: custom rules first, else catalog severity (as today) | closes `health-broken/<check>` |
| broken  | open/keep `health-broken/<check>` at **fixed Warning** | does **not** close `health/<check>` |
| skipped | none                                              | closes both, failure close message says "skipped" not "recovered" |
| absent  | none (trust the reporter, as today)               | closes both |

- warning and failed share the `health/<check>` ref — same check thread,
  the filed severity differs. warning→failed and back are events on the
  same issue.
- Broken is filed at fixed `Severity::Warning` (visible, below
  `OPENS_INCIDENT`), description `Health check '<check>' is broken`, message
  from `per_check_description` extras. It does not go through the rules
  engine; no per-check override column for now.
- **warning and failed** checks reach the rules engine. Severity
  resolution: custom rules ladder first (it can now condition on the
  result, see below); no match ⇒ fixed Warning for warning-result checks,
  catalog base severity for failed.
- Every well-formed check name seen — any result — still upserts a default
  catalog row.
- `check`/`healthy` stay reserved keys stripped from `check_extra`; the
  **normalised** result string is *injected* as `result` into the rule
  evaluation context (and samples), so custom rules can write
  `check.result == "warning"` — uniformly, even for legacy stored pushes
  where the wire field was `healthy`.

## Normalisation

One enum + one normalise-on-read helper, mirrored Rust/TS, used by every
reader (wire *and* stored rows):

- Rust: `CheckResult { Passed, Warning, Failed, Broken, Skipped }` in
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
  - `result`: string ∈ {passed, warning, failed, broken, skipped} —
    anything else 400;
  - `healthy`: bool (legacy).
  Both present ⇒ 400; neither ⇒ 400. Error messages name the entry index as
  today. Reuses `AppError::BadRequest`, no new error type (no ERRORS.md
  change).
- Replace `collect_failing_checks` with `collect_check_results(health) ->
  BTreeMap<String, CheckResult>` via `CheckResult::from_entry`; it also
  feeds the catalog upsert (any entry with a resolvable result).
- `file_health_events`:
  - warning/failed ⇒ open `health/<check>` at the resolved severity
    (rules → fixed Warning / catalog base, see Semantics), with the
    normalised result injected as `result` in `check_extra` and
    `check`/`healthy` stripped.
  - broken ⇒ open `health-broken/<check>` at fixed Warning,
    `description: "Health check '<check>' is broken"`.
  - closes are derived from the issues that are actually open
    (`Issue::active_refs_with_prefix`), not from diffing the previous
    status row — an issue can stay open across pushes that don't re-file
    it (failed → broken keeps the failure open), so the previous push
    alone can't tell what needs closing. An open `health/<check>` closes
    when the check is now passed, skipped, or unmentioned — message
    "recovered", or "skipped" when currently skipped; an open
    `health-broken/<check>` closes on any current result other than
    broken ("no longer broken"). Legacy stored rows interoperate because
    the current push is normalised.
- `HealthCheck` payload struct (openapi doc shape): `healthy` becomes
  `Option<bool>`, add `result: Option<CheckResult>`, document the
  exactly-one rule and per-state semantics.

### 2b. database — severity resolution (`healthcheck_severities.rs`)

- `HealthcheckSeverity::severity_for` takes the normalised `CheckResult`
  (warning or failed): rules ladder evaluates first as today; on no match
  the fallback is `Severity::Warning` for `CheckResult::Warning`, the
  row's base `severity` for `CheckResult::Failed`. Doc the contract.

### 3. database (`crates/database/src/statuses.rs`)

- `Status::health_state()`: an entry counts toward `Warning` when its
  normalised result is Warning, Failed, **or** Broken; Passed/Skipped
  don't.

### 4. private-server

- `fns/statuses.rs` `compute_check_severities`: select Warning + Failed
  entries via the normaliser (Broken doesn't need a computed severity —
  it's fixed Warning and the UI renders it from the result directly);
  inject the normalised `result`, strip `check`/`healthy`.
- `fns/healthchecks.rs` `sample`: inject the normalised `result` into
  `check_extra`, strip `check`/`healthy`.
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
  - sort: failed, warning, broken, skipped, passed; then by name.
  - `CheckIcon` states: passed → green tick (unchanged); warning and
    failed → computed-severity icon (unchanged mechanism, now covers
    warning entries too); broken → `BuildCircle` (warning colour),
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
  - warning files at fixed Warning by default; a custom rule on
    `check.result` overrides it; warning→failed stays on the same issue.
  - broken files Warning at `health-broken/<check>`; does not touch an open
    failure issue.
  - failed→broken: failure stays open, broken issue opens; broken→passed
    closes both.
  - skipped: files nothing, closes a prior failure (message mentions
    skipped) and a prior broken issue.
  - legacy prev-row (healthy:false) → new push (result:passed) closes.
- database tests: `health_state` with result-form entries (warning/broken
  ⇒ Warning rollup, skipped ⇒ Healthy); `severity_for` fallback per
  result kind; rules ladder matching on `check.result`.
- private-server tests: snapshot `check_severities` covers warning +
  failed; sample injects normalised `result`.
