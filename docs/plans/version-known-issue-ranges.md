# Version known-issues as ranges

## Background

`version_known_issues` currently attaches each issue to a single
`version_id`. A version is `ready` iff it has no open known issues.

We want issues to apply to a range of versions instead:

- When raised on `M.m.p`, the issue's lower bound is `M.m.p` (inclusive)
  and the upper bound is implicitly `M.(m+1).0` (start of the next minor)
  — i.e. it covers every later patch in the same minor.
- When resolved, the operator nominates the fix version `M.m.f`. The
  upper bound becomes `M.m.f` (exclusive). Patches before the fix remain
  affected ("not ready"); patches from `M.m.f` onwards are clean.

The minor-group ready badge in the admin UI should reflect the latest
patch's readiness within the minor (an issue on an older patch that was
fixed before the latest doesn't dim the whole minor).

The public site (artifact lists, update-for, HTML pages, downloads) only
exposes ready versions.

## Schema

New migration `2026-05-21-NNNNNN-0000_version_known_issue_ranges`:

- Add `min_major INT`, `min_minor INT`, `min_patch INT`, `NOT NULL`
  after backfill.
- Add `max_major INT`, `max_minor INT`, `max_patch INT`, nullable.
  All-or-nothing (CHECK).
- Backfill from existing rows:
  - `min_*` from the referenced `versions` row.
  - For unresolved rows: `max_*` stays NULL.
  - For resolved rows: `max_* = (min_major, min_minor, min_patch + 1)`
    — treat legacy resolves as "fixed in the very next patch". Imperfect,
    but the dataset is tiny (one production deploy) and reviewable.
- Drop `version_id`.
- Drop the existing indices that reference `version_id` and the
  `resolved_at`-based partial index, replace with:
  - `(min_major, min_minor, min_patch)` for range queries.
  - Partial `(min_major, min_minor)` `WHERE max_major IS NULL` for the
    "still-open within this minor" lookup.
- Add CHECKs:
  - `max_*` columns are all-NULL or all-NOT-NULL.
  - If `max_*` set: same minor as `min_*` (max_major = min_major AND
    max_minor = min_minor) and `max_patch > min_patch`.
  - Keep `resolved_consistency` constraint but tie `resolved_at IS NOT NULL`
    to `max_major IS NOT NULL` (and vice versa) — they always co-occur in
    the new flow.

Down migration: re-add `version_id` (nullable while migrating), best-effort
look up a version row matching `(min_major, min_minor, min_patch)`, then
drop the new columns. Since this is a one-way feature in practice, the
down migration is allowed to lose information (e.g. dropped rows for issues
whose min doesn't match a real version).

## Database model (`crates/database/src/version_known_issues.rs`)

Field layout:

```rust
pub struct VersionKnownIssue {
    pub id: Uuid,
    pub created_at: Timestamp,
    pub author: String,
    pub description: String,
    pub min_major: i32,
    pub min_minor: i32,
    pub min_patch: i32,
    pub max_major: Option<i32>,
    pub max_minor: Option<i32>,
    pub max_patch: Option<i32>,
    pub resolved_at: Option<Timestamp>,
    pub resolved_by: Option<String>,
    pub resolution_message: Option<String>,
}
```

Methods:

- `add(db, min: (i32, i32, i32), author, description)` — insert with
  `max_*` NULL.
- `resolve(db, issue_id, fix: (i32, i32, i32), resolved_by, message)` —
  set `max_*` and the resolved-* fields. Refuse if `fix.0/.1 != min`'s
  major/minor or if `fix.2 <= min_patch` (DB CHECK enforces; surface as
  `AppError::BadRequest`).
- `list_for_minor(db, major, minor)` — all rows whose `min_minor` matches;
  used by the version-detail page (we show every issue whose range touches
  this minor, even those that don't include this exact patch — operators
  benefit from full minor context).
- `affecting(db, major, minor, patch)` — rows whose range includes the
  given coordinates (used by the version-detail "ready" check).
- `affected_versions(db, ids: &[Uuid])` — set of version IDs from `ids`
  that are affected by any issue. Replaces `versions_with_open`. Computed
  via a join.

The "affects" predicate, as SQL:

```sql
k.min_major = v.major
AND k.min_minor = v.minor
AND v.patch >= k.min_patch
AND (k.max_patch IS NULL OR v.patch < k.max_patch)
```

## Private-server (`crates/private-server/src/fns/versions.rs`)

Wire shapes:

- `KnownIssueData`: replace nothing, add `min_major`, `min_minor`,
  `min_patch`, `max_major`, `max_minor`, `max_patch`. (The UI will compose
  "Affects 2.52.6+" / "Affects 2.52.6 – 2.52.8 (fixed in 2.52.9)".)
- `MinorVersionGroup`: add `ready: bool`. True iff the latest *published*
  patch in the minor is not affected (or if there are no published
  patches, default true).
- `VersionDetail`: unchanged externally (it still has `ready` and
  `known_issues`).

Endpoint changes:

- `add_known_issue` keeps `version_id` as input — look up the version's
  coords, insert with `min_* = coords`. (Simpler UX: the "Add" button is
  on a version's detail page; min comes from there.)
- `resolve_known_issue` gains `fix_version: VersionStr` (string parsed via
  `VersionStr::from_str`). Reject if fix isn't in the same minor or isn't
  strictly above min.
- `list_known_issues` keeps `version_id` input; return issues affecting
  that minor (use `list_for_minor`), so the UI sees the full picture.
- `get_grouped_versions` computes per-minor `ready` by checking whether
  the latest-patch's id is in the affected set.
- `get_version_detail`'s `known_issues` switches to `list_for_minor` (same
  rationale), but `ready` is still computed from issues that affect this
  exact patch.

## Public-server (`crates/public-server/src/versions.rs`)

Filter to "ready" everywhere the public sees versions:

- `list` (GET `/`): return only versions not in `affected_versions(all_ids)`.
- `update_for`: filter the returned `ViewVersion` set by ready.
- `view_artifacts` / `view_mobile_install` / `list_artifacts` /
  `download_artifact`: when resolving a range to a concrete version,
  pick the latest *ready* match. If the requested version is exact and
  not ready, 404 (consistent with "only ready is displayed").
- `server_versions.rs` and any private-server caller of
  `get_latest_matching` keep current behaviour — those are admin contexts.

Approach: add `Version::get_latest_matching_ready` and have the public
endpoints call that. For `list` and `update_for`, fetch first then filter
the returned slice using `VersionKnownIssue::affected_versions`. No
changes to private-server callers of the existing helpers.

`get_latest_matching_ready` implementation: load matching versions
ordered desc as now, in-memory join with the affected set, then `.find()`
on range satisfaction.

## Frontend (`private-web/`)

`types.ts` re-exports stay aligned with regenerated `api-types.ts`.

`Versions.tsx`:

- Render a `ReadyChip` next to the version string in each `MinorGroup`
  summary row, based on `group.ready`.

`VersionDetail.tsx`:

- `KnownIssueRow`: show range string under the description ("Affects
  2.52.6 and later" when `max_major == null`, otherwise "Affects 2.52.6 –
  2.52.8 (fixed in 2.52.9)").
- `ResolveKnownIssueDialog`: add a "Fix version" text field. Default to
  the current page's version (sensible if the operator is resolving on
  the page of the fix). Validate same-minor / above-min client-side and
  let the server enforce too.

No new dialogs or routes.

## Tests

Update `crates/private-server/tests/version_known_issues.rs`:

- Existing flows continue to assert ready/not-ready and the resolve
  round-trip.
- Add: resolving with a fix version narrows the range — older patches
  remain not-ready, fix and later are ready.
- Add: minor-group ready follows the latest patch.
- Add: 400 when resolve fix is in a different minor / not above min.

New (or extended) public-server test:

- `crates/public-server/tests/versions.rs`: a non-ready version is
  excluded from `list` and `update_for`.

## Migration ordering

Single migration file does schema + backfill + drops. Diesel migrations
run in order; the next `just migrate` picks it up. Schema regen via
`diesel print-schema` (the codebase already has `crates/database/src/schema.rs`
committed; regenerate after migrating).

## Out of scope

- Backporting fixes across minors (each minor's known issues are
  independent).
- Editing an existing issue's min/max after creation (operators can
  resolve & re-raise).
- A bulk "raise across versions" UI.
