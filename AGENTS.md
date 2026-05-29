# Agent Rules for canopy

Avoid writing large summaries of actions taken when done.

## Project Structure Overview
- **Database crate**: Models, migrations, and database logic
- **Public server**: Internet-exposed API endpoints for device registration and updates
- **Private server**: Admin HTTP/JSON API in axum, plus an embedded React SPA (in `private-web/`) served at the root in production
- **Commons**: Shared utilities, authentication, error handling

## Database Connection
- Database models are in `crates/database/src/` with re-exports in `lib.rs`
- Use migrations in `migrations/` directory for schema changes
- Use `just migrate` to run migrations
- Use PostgreSQL native functions where possible (e.g., `position()` for binary substring searches)
- Complex queries: Prefer database-level operations over in-memory filtering

## Code Style & Patterns
- Follow existing Rust conventions in the codebase
- Never add useless comments if what the code does is obvious enough, especially in CSS
- Use `AppError` variants for error handling, map to appropriate HTTP status codes in `IntoResponse`
- Update ERRORS.md when adding new error types, the heading must match the error problem type
- Use `commons_tests::db::TestDb::run()` for database-only tests
- Use `commons_tests::server::run()` for HTTP endpoint tests
- Use `commons_tests::server::run_with_device_auth()` for authenticated device tests in the public server
- Admin endpoints take a `TailscaleAdmin` axum extractor; the React UI gates with `commons.is_current_user_admin`

## Private server architecture
- **Server fns** under `crates/private-server/src/fns/<module>.rs` are bare axum handlers with `(State, [auth extractor], Json<Args>) -> Result<Json<T>>` signatures.
- Each module exposes `pub fn routes() -> Router<AppState>` and is mounted under `/api/<module>` by `crate::fns::routes()`.
- The SPA fallback (`crate::spa::handler`) serves the embedded React bundle from `private-web/dist/` for any path the API doesn't claim.
- `build.rs` runs `npm install --frozen-lockfile && npm run build` in `private-web/` before embedding. Set `SKIP_FRONTEND_BUILD=1` to skip (`just`'s recipes already do this for dev workflows).

Example axum handler pattern:
```rust
#[derive(Deserialize)]
pub struct AddArgs { pub email: String }

pub async fn add(
    State(state): State<AppState>,
    TailscaleAdmin(_): TailscaleAdmin,
    Json(args): Json<AddArgs>,
) -> Result<Json<()>> {
    let mut conn = state.db.get().await?;
    database::admins::Admin::add(&mut conn, &args.email).await?;
    Ok(Json(()))
}
```

## Testing Patterns
- Use `#[tokio::test(flavor = "multi_thread")]` for async tests
- Database tests: `commons_tests::db::TestDb::run(|mut conn, _url| async move { ... })`
- HTTP tests: `commons_tests::server::run(|conn, public, private| async move { ... })`
- Device auth tests: `commons_tests::server::run_with_device_auth("role", |conn, cert, device_id, public, private| async move { ... })`
- In public-server, for authenticated tests, add `mtls-certificate` header: `.add_header("mtls-certificate", &cert)`
- In public-server, use `.add_header("mtls-certificate", &cert)` on test requests, do not set it on `public` or `private` server directly (these should not be `mut` in tests)
- Test both success and error scenarios (especially 404 cases for non-existent resources)
- For database tests, use direct model functions instead of HTTP endpoints
- Always include `use database::ModelName;` imports in test files
- Do not include `_test` suffix or prefix in test filenames in `tests/` directory
- Calling private-server endpoints in tests:
  - Endpoints are at `/api/<module>/<function>` (e.g. `/api/statuses/server_grouped_ids`)
  - Pass parameters via `.json(&serde_json::json!({"param_name": value}))`
  - For functions with no parameters, still send an empty body: `.json(&serde_json::json!({}))`

## React frontend (`private-web/`)
The React + MUI + Vite frontend lives at `/private-web/` and is embedded into the private-server binary at build time via `rust-embed`.

Local dev workflow (two terminals):
- `just watch-private-api` runs the private-server binary on `127.0.0.1:8081`. (We bind to IPv4 because Node's vite-proxy can't resolve `[::1]` literals.) `SKIP_FRONTEND_BUILD=1` is set so `cargo run` doesn't reinvoke `npm run build` on every iteration.
- `just watch-private-web` runs Vite at `:8090`, proxying `/api` to the API.

Open `http://localhost:8090/`. The Vite proxy makes the React app same-origin with the API, so no CORS plumbing is needed.

### Wire types are generated from the Rust spec

`private-web/src/api-types.ts` is **generated** from `private-web/openapi.json`,
which is in turn generated from the `#[utoipa::path]` annotations on the
private-server handlers. Both files are checked in so fresh checkouts work
without a Rust build.

Run `just gen-openapi` after changing any private-server handler's request
body, response body, security scheme, or tag. The recipe rebuilds the
`openapi-dump` binary, writes `private-web/openapi.json`, then runs
`openapi-typescript` (via `npm run gen:api-types`) to regenerate
`private-web/src/api-types.ts`. Commit both files alongside the Rust change.

`private-web/src/types.ts` is a hand-written thin re-export layer over
`api-types.ts`. Most consumer code imports from `../types`; new code can do
the same. The file also holds UI-only constants (severity ordering, label
maps, etc.) that don't belong in the wire spec.

End-to-end tests use Playwright. Run with `npm run test:e2e` from `/private-web/`. The fixture (`e2e/fixture.ts`) spawns its own private-server + Vite pair against a freshly-migrated `canopy_e2e_<random>` Postgres database per worker, so the operator does not need to keep `just watch-private-api` running. Build the binaries first with `cargo build --bin private-server --bin migrate`. Override the admin connection used to create/drop the throwaway DB with `CANOPY_E2E_ADMIN_DATABASE_URL` (default `postgres://localhost/postgres`); set `CANOPY_E2E_VERBOSE=1` to stream backend/frontend logs. The first run on a fresh checkout needs `npx playwright install chromium`.

## Development Workflow
- Always check: `just check` for basic compilation
- Run full test suite: `just test`
- Run specific tests: `just test-name <test_name>`
- Verify no compilation warnings in tests and main code

### Tests run on a throwaway RAM-backed Postgres by default
Each test creates and drops its own database (and runs every migration), and
nextest runs many in parallel. Against a disk-backed Postgres the resulting
`CREATE DATABASE`/`DROP DATABASE` fsync storm saturates disk I/O and can make
the whole machine unresponsive.

So `just test` (and `test-package`/`test-name`/`test-verbose`/`test-e2e`) run
through `scripts/ramdisk-pg.sh`, which spins up a disposable Postgres on tmpfs
(`/dev/shm`) with `fsync`/`synchronous_commit`/`full_page_writes` off, points
`DATABASE_URL` at it, then tears it down. Nothing touches a physical disk, so
there's no grind and runs are dramatically faster. It reuses your installed
`initdb`/`pg_ctl`, so the server version matches your system Postgres with no
container or image to manage. `just test` takes nextest args, so `just test`,
`just test -p database`, and `just test <name>` all work. Wrap other commands
with `just fast <cmd>`.

Requirements/caveats: needs the Postgres *server* tools (`initdb`/`pg_ctl`), not
just the `psql` client. On macOS there's no `/dev/shm`, so it falls back to disk
— still fast (fsync is off), just not RAM-backed unless you point
`CANOPY_TEST_PG_DIR` at a real ramdisk. Other overrides: `CANOPY_TEST_PG_PORT`,
`CANOPY_TEST_PG_ROLE`.

To run against your **system** Postgres instead (to inspect the DB afterwards,
or where `initdb` isn't available), use `just test-system [nextest args]` —
prefix with `nice` to soften the I/O grind.

## Version Control
- If the working copy is a jujutsu repo (a `.jj` directory exists at the repo root), prefer `jj` commands over `git` for VCS operations (status, diff, log, commit/describe, etc.). The repo may be colocated with git, but `jj` is the source of truth for local work.
- If there is no `.jj` directory, use `git` as normal.

## Troubleshooting and common mistakes
- Always use just for tests, never use `cargo test`.
- Unless you've done wide-ranging changes, prefer to test specific packages with `just test-package <package_name>` instead of the full test suite.
- For frontend type checking, use `just typecheck`, not `npx tsc` at the repo root. There's no `package.json` at the root so the bare `tsc` invocation silently no-ops and you get false confidence that types are fine.
- If you're trying to import diesel in the private-server crate, stop that and put database stuff in the database crate instead.
- When files change from under you (ie when the dev changes things without telling you), assume those changes are intentional instead of reverting them.
- If you're not sure, STOP AND ASK.
