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

End-to-end tests use Playwright. Run with `npm run test:e2e` from `/private-web/`. Tests start Vite themselves but assume the operator already has `just watch-private-api` running for the backend. The first run on a fresh checkout needs `npx playwright install chromium`.

## Development Workflow
- Always check: `just check` for basic compilation
- Run full test suite: `just test`
- Run specific tests: `just test-name <test_name>`
- Verify no compilation warnings in tests and main code

## Version Control
- If the working copy is a jujutsu repo (a `.jj` directory exists at the repo root), prefer `jj` commands over `git` for VCS operations (status, diff, log, commit/describe, etc.). The repo may be colocated with git, but `jj` is the source of truth for local work.
- If there is no `.jj` directory, use `git` as normal.

## Troubleshooting and common mistakes
- Always use just for tests, never use `cargo test`.
- Unless you've done wide-ranging changes, prefer to test specific packages with `just test-package <package_name>` instead of the full test suite.
- If you're trying to import diesel in the private-server crate, stop that and put database stuff in the database crate instead.
- When files change from under you (ie when the dev changes things without telling you), assume those changes are intentional instead of reverting them.
- If you're not sure, STOP AND ASK.
