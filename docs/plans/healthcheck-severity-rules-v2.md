# Healthcheck severity catalog v2: context-conditional rules

## Context

v0 retired bestool's overall-health rollup; v1 made per-check severity an operator-owned catalog (`healthcheck_severities`, default Warning, pending-review state, top-level `/healthchecks` page). v2 layers per-check **conditional rules** on top of the base catalog severity.

A check's whole rule set lives in a single `rules JSONB` column on `healthcheck_severities`. The stored value is a **JsonLogic `if`-ladder** that evaluates to a Severity string:

```jsonc
{ "if": [
    <condition_1>, "<severity_1>",
    <condition_2>, "<severity_2>",
    ...,
    <condition_N>, "<severity_N>"
] }
```

Evaluation returns the first matching branch's severity. If no branch matches, the ladder returns null and canopy falls through to the catalog row's `severity` column (the v1 base). When `rules` is NULL, no evaluation happens — the catalog base is used directly (also the v1 behaviour). **No per-rule notes**: the storage is one JsonLogic value, no wrapping struct.

**No composition** in conditions — each condition is exactly one operator (no AND/OR/NOT nesting inside a condition). The constrained shape, plus the constrained `if` structure (even-length args, every operand is either a condition or a severity literal), is enforced by the typed Rust deserialiser. JsonLogic-as-storage gives us a syntax that's diffable and language-agnostic, with room to lift the no-composition restriction later by relaxing the deserialiser; no migration needed.

The `var` path inside a condition uses three namespaces:
- `check.<field>` — the failing check's own fields (entry inside `statuses.health[i]`, less `check` and `healthy`).
- `status.<field>` — top-level status extras (`statuses.extra` — bestool sends `bestoolVersion`, `tamanuVersion`, `uptimeSecs`, etc).
- `tag.<key>` — the failing server's resolved tag map (merged server + group tags, same resolution as `/api/tags`).

Note: the `X-Version` request header (which lands on `statuses.version`) is the **tamanu** version, not bestool's, and is not reliable enough to predicate on. The plan deliberately does not surface it as a `var` namespace. Use `status.tamanuVersion` for tamanu-version predicates, `status.bestoolVersion` for the bestool side.

## Schema

A small migration adds a nullable `rules` column to the v1 catalog table — no new table, no FK.

`migrations/<date>_healthcheck_severity_rules/up.sql`:

```sql
ALTER TABLE healthcheck_severities
    ADD COLUMN rules JSONB;  -- NULL ⇒ no conditional rules, use the row's severity column.
```

When non-NULL, the column holds a JsonLogic `if`-ladder of the form `{"if": [c1, s1, c2, s2, …, cN, sN]}` — even number of arguments, every odd index is a condition (single op, no composition), every even index is a Severity string. No trailing else: the JsonLogic returns null on no-match, and the Rust evaluator falls through to the row's `severity` column. Storing the ladder this way means the operator-set "base severity" lives in exactly one place (the catalog column) regardless of whether the check has rules.

Validation lives in Rust at the API layer — see §Predicate model — so the column stays a plain JSONB with no shape check (operators editing SQL directly can write garbage; the evaluator falls through to the base severity on parse failure, log + ignore).

## Predicate model

Typed Rust shape:

```rust
/// The whole `rules` column when non-NULL.
pub struct IfLadder {
    /// At least one branch — empty ladders are normalised to NULL at save time.
    pub branches: Vec<(Condition, Severity)>,
}

pub enum Condition {
    Eq(Var, Literal),
    Neq(Var, Literal),
    Lt(Var, Literal),
    Lte(Var, Literal),
    Gt(Var, Literal),
    Gte(Var, Literal),
    InRange(Var, String /* semver range */),
}

pub struct Var { pub kind: VarKind, pub field: String }

#[derive(Clone, Copy)]
pub enum VarKind { Check, Status, Tag }

pub type Literal = serde_json::Value;  // number | string | bool
```

Custom `Serialize` / `Deserialize` impls map between Rust and JsonLogic:

| JsonLogic op | Condition variant | Semantics |
|---|---|---|
| `==` | `Eq` | numeric-coercion fast path; `"5"` == `5` is true |
| `!=` | `Neq` | inverse of `==` |
| `<`, `<=`, `>`, `>=` | `Lt`/`Lte`/`Gt`/`Gte` | numeric; both sides may be numbers or numeric-coercible strings; else false |
| `in_range` | `InRange` | non-standard JsonLogic extension; LHS string parses as `node_semver::Version`, RHS string as `node_semver::Range`; `range.satisfies(&version)`; missing or unparseable → false |

`IfLadder` (de)serializes as `{"if": [cond1, sev1, cond2, sev2, …]}` — even-length args, alternating conditions and severities. Deserialisation refuses:
- odd-length arg lists (a trailing else),
- non-condition operands at even indices (e.g. nested `if`, literal values),
- non-string operands at odd indices,
- unknown ops anywhere (`and`, `or`, `!`, `if` nested inside a condition, `map`, etc.),
- malformed `{"var": ...}` payloads.

Any of these → 400 at the admin API layer. At read time (ingestion path), a parse failure logs and falls through to the catalog severity — we never crash a status push on bad rules.

`Var` (de)serializes inside the JsonLogic `{"var": "..."}` wrapper as the dotted string `"check.<field>"` / `"status.<field>"` / `"tag.<key>"`. Regex check at parse time: `^(check|status|tag)\.[A-Za-z0-9_]+$`.

Missing field always evaluates the condition to **false** (the rule's branch doesn't fire), never an error. Same for type mismatch — see the op table above.

### Status extras shape

For reference, a real status.extra blob (from a Kiribati prod central):

```jsonc
{
  "arch": "x86_64",
  "bestoolVersion": "1.13.0",            // semver-parseable string → in_range works
  "canonicalUrl": "https://...",
  "currentSyncTick": "21608625",         // string-of-digits → numeric coercion applies
  "filesystems": [ { "fsType": "NTFS", "mountpoint": "C:\\" } ],  // ARRAY — out of scope for v2
  "hostname": "KIRIBATI-PROD-CENTRAL",
  "ipv4": true, "ipv6": false, "nat64": false, "virtualised": false,  // bools → Eq/Neq
  "kernel": "17763",                     // string-of-digits
  "osKind": "windows",
  "osName": "Windows",
  "osTimezone": "Pacific/Fiji",
  "osVersion": "10 (17763)",             // NOT semver → in_range would be false
  "pgVersion": "PostgreSQL 17.6 ...",    // free-form text → Eq only
  "tamanuVersion": "2.48.1",             // semver-parseable
  "timezone": "Pacific/Nauru",
  "uptimeSecs": 6038594                  // JSON number
}
```

Predicatable: every flat scalar field above (string, number, bool). **Not predicatable in v2**: nested structures (`filesystems[]`, any future array/object value). Would need either jq-style paths or a dedicated nested-var kind, deferred until there's a concrete use case.

Evaluation entry point lives next to the existing catalog model:

```rust
// crates/database/src/healthcheck_severities.rs
pub struct EvaluationContext<'a> {
    pub status_extra: &'a serde_json::Map<String, serde_json::Value>,
    pub check_extra:  &'a serde_json::Map<String, serde_json::Value>,
    pub tags:         &'a std::collections::HashMap<String, serde_json::Value>,  // pre-wrapped
}

impl Var {
    pub fn resolve<'a>(&self, ctx: &'a EvaluationContext<'a>) -> Option<&'a serde_json::Value> {
        match self.kind {
            VarKind::Check  => ctx.check_extra.get(&self.field),
            VarKind::Status => ctx.status_extra.get(&self.field),
            VarKind::Tag    => ctx.tags.get(&self.field),
        }
    }
}

impl Condition { pub fn matches(&self, ctx: &EvaluationContext) -> bool { /* per-variant */ } }

impl IfLadder {
    /// First matching branch's severity, or None when nothing matches.
    pub fn evaluate(&self, ctx: &EvaluationContext) -> Option<Severity> {
        self.branches.iter().find_map(|(c, s)| c.matches(ctx).then_some(*s))
    }
}
```

(Implementation detail: tag values are `String`s but `Var::resolve` returns `Option<&JsonValue>` for uniform comparison. The context-build step wraps each tag string in `Value::String` so the comparison path doesn't need a special branch.)

`HealthcheckSeverity::severity_for` is extended to take an `EvaluationContext`. It reads the same catalog row it already does; if `rules` is non-NULL it parses to an `IfLadder` and calls `evaluate` — using the returned severity if `Some`, otherwise falling through to the row's `severity` column. Parse failures on the stored JsonLogic are logged and degrade to the base severity. One row read, no second query.

### Worked examples

Each blob below is the **entire** `rules` value for that check.

```jsonc
// 1. tamanu_service broken in bestool 2.4.0–2.5.3 → warning. Otherwise the base.
{ "if": [
    { "in_range": [{ "var": "status.bestoolVersion" }, ">=2.4.0 <2.5.4"] }, "warning"
] }

// 2. disk_space.used_pct > 95 → critical. Otherwise the base.
{ "if": [
    { ">": [{ "var": "check.used_pct" }, 95] }, "critical"
] }

// 3. cert_expiry tiered: error <7d, warning <30d. Otherwise the base.
{ "if": [
    { "<": [{ "var": "check.days_remaining" }, 7] },  "error",
    { "<": [{ "var": "check.days_remaining" }, 30] }, "warning"
] }

// 4. Server-tag predicate: raise this check to error only on production servers.
{ "if": [
    { "==": [{ "var": "tag.environment" }, "prod"] }, "error"
] }
```

Combined predicates ("bestool ≥ 2.6 AND used_pct > 95") aren't expressible inside a single condition — see §Out of scope.

## Ingestion change

`crates/public-server/src/statuses.rs:204` currently calls `HealthcheckSeverity::severity_for(conn, check)`. Updated call builds an `EvaluationContext` from the status row plus the server's resolved tags, then passes it through:

```rust
let entry = find_health_entry(&status.health, check); // existing helper
let check_extra = entry
    .map(|e| {
        let mut m = e.clone();
        m.remove("check");
        m.remove("healthy");
        m
    })
    .unwrap_or_default();
let status_extra = status.extra.as_object().cloned().unwrap_or_default();
let tags = resolved_tags_for(&mut db, &server).await?; // server + group merge
let ctx = EvaluationContext {
    status_extra: &status_extra,
    check_extra:  &check_extra,
    tags:         &tags,
};
let severity = HealthcheckSeverity::severity_for(conn, check, &ctx).await?;
```

`status.extra` is the top-level JSONB column (see `crates/database/src/statuses.rs`) — the object holding `bestoolVersion`, `tamanuVersion`, etc. `status.version` (the X-Version header — tamanu's version, not bestool's, and unreliable) is deliberately unused; tamanu-version predicates use `status.tamanuVersion` which bestool always sends.

Tag resolution reuses the same merge logic as the existing public-server `/api/tags` endpoint (`crates/public-server/src/tags.rs`) — server tags overlay group tags. The resolved map is fetched once per push (not once per failing check) and shared across all rule evaluations for that push. If no relevant rules use `tag.*` we still pay the small cost — acceptable; the map is tiny.

## Admin API

One new endpoint, plus an extension of `list_severities` to surface the new column. All `TailscaleAdmin`-gated; operationIds prefixed `healthcheck_` per the convention established in #186.

- `list_severities` is extended to include `rules: Option<IfLadder>` (the parsed contents of the JSONB column, or null) and `rule_count: u32` (the number of branches; 0 when `rules` is null). The catalog UI uses `rule_count` for the main-page branching (dropdown vs Custom rules link); the per-check page uses `rules` directly without a second fetch.
- `POST /api/healthchecks/update_rules` — `{ check_name, rules: Option<IfLadder> }` → updated `HealthcheckSeverityData`. Full replacement: the entire ladder is rewritten. Sending `null` clears the column; sending an empty-branches ladder is normalised to null. Validation runs through the typed deserialiser; malformed → 400 with details. Stamps `reviewed_at` + `reviewed_by` like `update_severity` does (editing rules counts as a review for the row).

`update_severity` is unchanged — still edits only the base `severity` (+ notes / reviewed metadata). The base severity is also the no-match fallback for any `if`-ladder, so operators can freely tune it from either the main page or the per-check page; both write the same column.

Regenerate `private-web/openapi.json` + `private-web/src/api-types.ts` via `just gen-openapi`.

## React UI

Two surfaces. The main `/healthchecks` page keeps its simple-case feel; the per-check sub-page hosts the rules editor.

### Main page (`/healthchecks`, edit in place)

For each catalog row:
- **If `rule_count == 0`**: severity column renders the same Select dropdown as today (8 severities + Save). One extra menu entry below a divider: `Custom rules…` — selecting it persists nothing on its own, just navigates to `/healthchecks/<check_name>`.
- **If `rule_count > 0`**: severity column renders a non-dropdown affordance — a "Custom rules (N) · base: <severity chip>" link that goes to the sub-page. The plain dropdown is hidden because in this mode the *effective* severity is per-push; surfacing only the base would be misleading. Operators who want to remove all rules and go back to the dropdown do so on the sub-page (which has a "Delete all rules" action) and the main page returns to its simple form.

### Per-check sub-page (`/healthchecks/<check_name>`)

- **Header**: check name, "first seen" TimeAgo, "reviewed by" metadata, a "Back to list" link.
- **Base severity** card: same Save-button affordance as the main page row, scoped to this check. Written to the catalog row's `severity` column. This is the fallback used when no rule branch matches *and* the only severity shown when the check has no rules.
- **Notes** card (new in v2; the main page never exposed this): a multiline `TextField` populated with the catalog row's `notes`, plus a Save button calling `update_severity` (which already accepts `notes`). Used for operator commentary on the check itself; separate from per-rule context, which lives… nowhere (per-rule notes were deliberately dropped — see §Out of scope).
- **Rules** section: ordered table with columns `#` (position), `Condition` (rendered as `<var> <op-symbol> <value>`), `Severity`, and an actions column (edit, delete, up/down arrows). Empty state: "No rules — the base severity above is used for every push."
- **Add rule** button → dialog with four inputs (no notes):
  - `Var` text input (free-form; submit-time regex `^(check|status|tag)\.[A-Za-z0-9_]+$`).
  - `Op` select (`=`, `≠`, `<`, `≤`, `>`, `≥`, `version in`).
  - `Value` input (text; for the numeric ops the form tries to parse it as a number on save, falling back to string if parse fails — operators don't have to think about JSON quoting).
  - `Severity` select.
- All rule edits accumulate in local state, then a single **Save rules** button fires `update_rules` with the rebuilt `IfLadder`. Reorder via up/down arrows is a local-state mutation; the same Save commits.
- **Delete all rules** button (separate from individual delete): clears the local array; Save commits, which sends `rules: null` so the column is cleared.

Non-admins see all three editors in read-only mode.

Condition rendering for the table cell maps JsonLogic op symbols to displayable ones: `==`→`=`, `!=`→`≠`, `<`/`<=`/`>`/`>=` as-is, `in_range`→`∈`. Examples:
- `status.bestoolVersion ∈ ">=2.4.0 <2.5.4"`
- `check.used_pct > 95`
- `tag.environment = "prod"`

The `var` input is free-form rather than a two-step dropdown (kind: check|status|tag, then field name) — bestool's extras and operator tags both grow over time and operators can introduce new ones the moment they exist, without UI changes.

New files: `private-web/src/routes/HealthcheckDetail.tsx`; reuses `SeverityChip`, `TimeAgo`, `useApi`, `useApiAction`, `useIsAdmin`, `usePageTitle`. New route registered in `private-web/src/App.tsx`: `<Route path="/healthchecks/:checkName" element={<HealthcheckDetail />} />`. The main `Healthchecks.tsx` is edited to consult `rule_count` and branch between the dropdown and the link affordance.

## Tests

Database (`crates/database/tests/healthcheck_severity_rules.rs`, new):
- `IfLadder` round-trips through the constrained JsonLogic shape.
- Deserialisation rejects: odd-length args, non-condition operand at an even index, non-string severity at an odd index, unknown condition op, malformed `{"var": ...}`, AND/OR/NOT/nested `if`.
- `Var` (de)serialises as the dotted string for `check.*`, `status.*`, `tag.*`; rejects unknown kinds and missing dots.
- `in_range` matches inside the range, misses outside; non-semver LHS (e.g. `osVersion: "10 (17763)"`) → false; missing field → false.
- Numeric ops with numeric LHS, with numeric-coercible string LHS (`"21608625"`), with non-numeric string LHS (→ false), with missing field (→ false).
- `==` / `!=` on bool, string, numeric values.
- Tag-namespace var resolves against the tag map, missing key → false.
- `severity_for` returns the first matching branch's severity; falls back to the row's `severity` column when no branch matches AND when `rules` is NULL AND when `rules` fails to deserialise.

Public server (`crates/public-server/tests/statuses.rs`, extend):
- A status push carrying `bestoolVersion` inside a ladder's range files the per-check issue at the matched severity, not the base.
- A push with the check's extra field crossing a threshold files at the elevated severity; below-threshold pushes file at the base.
- Tag predicate: a server with `tag.environment=prod` triggers a branch that wouldn't fire on a `tag.environment=staging` server.
- Tiered: a two-branch ladder (`<7` then `<30`) correctly differentiates a push with `days_remaining: 3` from `15`.

Private server (`crates/private-server/tests/healthcheck_rules.rs`, new):
- `update_rules` accepts a valid ladder; rejects unknown ops, malformed `var`, bad semver range, unknown severity, composition shapes.
- `update_rules` with `rules: null` clears the column.
- `update_rules` with an empty-branch ladder normalises to null.
- `list_severities` returns `rules` inline plus `rule_count`.
- Admin gate.

## Critical files

- `migrations/<date>_healthcheck_severity_rules/{up,down}.sql` (new — `ALTER TABLE healthcheck_severities ADD COLUMN rules JSONB ...`)
- `crates/database/src/healthcheck_severities.rs` (extend struct with `rules: Option<IfLadder>`; add `IfLadder`/`Condition`/`Var`/`VarKind`/`EvaluationContext`; change `severity_for` signature)
- `crates/database/src/schema.rs` (regenerated)
- `crates/public-server/src/statuses.rs` (line ~204: build context including resolved tags, pass to severity_for)
- `crates/public-server/src/tags.rs` (extract the tag-resolution helper if it isn't already callable from outside the handler)
- `crates/private-server/src/fns/healthchecks.rs` (add `update_rules`; extend `list_severities` response shape)
- `private-web/src/routes/HealthcheckDetail.tsx` (new — base severity, notes, rules editor)
- `private-web/src/routes/Healthchecks.tsx` (branch row severity column on `rule_count`)
- `private-web/src/App.tsx` (register route)
- `private-web/openapi.json`, `private-web/src/api-types.ts` (regenerated)
- Test files as above.

## Verification

- `nice just check`, `nice just test-package database`, `nice just test-package public-server`, `nice just test-package private-server`, `nice just typecheck`.
- `nice just gen-openapi` ran clean and both regenerated files committed.
- Manual via dev stack: push a status with `bestoolVersion: "1.4.2"` in the body's extras, see the failing check filed at base severity. Add a ladder `{"if": [{"in_range": [{"var": "status.bestoolVersion"}, ">=1.4.0 <1.5.0"]}, "warning"]}` via the per-check page → Warning. Push again — issue filed at Warning. Change `bestoolVersion` to `"1.5.0"`, push — issue back at base severity.
- Extra-field path: ladder `{"if": [{">": [{"var": "check.used_pct"}, 90]}, "critical"]}`. Push with `used_pct: 95` → Critical filing; with `used_pct: 50` → base severity.
- Tag path: tag the server with `environment=prod`, ladder `{"if": [{"==": [{"var": "tag.environment"}, "prod"]}, "error"]}`. Push fails on prod → Error filing; retag to `staging`, push → base severity.

## Out of scope / call-outs

- **Per-rule notes**: dropped. The storage shape is JsonLogic with no wrapping struct, so there's nowhere to put a per-rule comment without re-introducing one. Operators wanting to explain *why* a check has the rules it does use the catalog row's `notes` field (now editable on the per-check page).
- **AND/OR/NOT inside a condition**: rules are single-condition. JsonLogic *can* express composition; we deliberately don't. Lifting this is a deserialiser change (allow `and`/`or`/`!` shapes inside a condition operand of the `if`) — no schema migration needed. Defer until there's a concrete use case the workaround (multiple branches in order) can't cover.
- **Per-server / per-group rules**: rules apply globally per check name. The new `tag.*` namespace gives operators an "only on prod" lever (tag servers / groups and predicate on the tag), so the per-group severity floor in `docs/plans/issues-followups.md` item 5 is now even more orthogonal.
- **Cross-check predicates** (e.g. "raise cert_expiry when http is also failing"): not in v2; evaluation stays local to a single check at a time.
- **Nested path access** for arrays/objects in extras (e.g. `status.filesystems[0].mountpoint`): not in v2; defer until a real use case turns up.
- **Migrating existing operators' severities into rules**: not needed; rules are purely additive on top of the catalog's base severity.
