# Sources page with confirmations

Scenarios verifying the sources table's move to its own page and the
confirmation gate on every source-policy change.

## Navigation

- [x] The healthcheck catalog page no longer embeds the sources table and offers a "Manage sources" link out to it — verifies spec: CHK
- [x] The link opens `/settings/healthchecks/sources`, showing the sources table and the description
- [x] The Settings tab bar keeps "Healthchecks" selected while on the sources sub-page
- [x] A back link returns from the sources page to the catalog

## Confirmations

- [x] Changing a source's reachability mode opens a dialog naming the source and target mode, and only persists on Confirm — verifies spec: CHK
- [x] Changing a source's ingest mode opens the same confirmation, and persists on Confirm — verifies spec: CHK
- [x] Cancelling the confirmation leaves the mode and stored policy untouched — verifies spec: CHK
- [x] The dialog copy states the consequence of the chosen mode (warns/quiet/off; allow/ignore/deny) — verifies spec: CHK

## Preserved behaviour

- [x] After ingest is set to a non-allow mode, the reachability control is disabled — verifies spec: CHK
- [x] Non-admin operators see the modes as read-only chips, with no toggles or confirmation — covered via the new `nonAdminPage` fixture

## Test harness: non-admin coverage

The e2e auth bypass authenticated every caller as an admin, leaving the
non-admin view untestable. A dev-only request header now downgrades the bypass
to a non-admin identity, exercised end-to-end by the `nonAdminPage` fixture.

- [x] The dev non-admin header makes `is_current_user_admin` report false
- [x] The dev non-admin header makes an admin-gated endpoint reject with 403
