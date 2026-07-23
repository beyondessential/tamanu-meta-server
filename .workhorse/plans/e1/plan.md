# Move sources table to a separate page with confirmations

The sources table currently lives inline on the Healthchecks settings page
(`/settings/healthchecks`), rendered by `SourcesSection`/`SourceRow` in
`Healthchecks.tsx`. Its reachability/ingest toggles change source policy
immediately on click — high-danger, rarely-used, and easy to hit by accident.

## Approach

- Move the sources table to its own sub-page at `/settings/healthchecks/sources`,
  mirroring the existing `HealthcheckSettings` sub-page pattern (back link "←
  All healthchecks"; the Settings tab bar keeps "Healthchecks" highlighted via
  `valueFromPath`'s `startsWith("/settings/healthchecks")`).
- Link forward to it from the Healthchecks catalog page with a "Manage sources"
  entry point.
- Every reachability/ingest change opens a confirmation dialog naming the source,
  the target mode, and its consequence, before the change is applied. Cancel
  leaves the policy untouched; the toggle only moves on a confirmed success.

## Steps

- [x] Add `SourcesSettings.tsx` route component: back link, heading, the moved
      description, and the sources table
- [x] Move `SourceRow` there and gate each mode change behind a confirm dialog
      (`ConfirmModeDialog`) with per-mode consequence copy derived from CHK
- [x] Replace the inline `SourcesSection` in `Healthchecks.tsx` with a "Manage
      sources" link/button
- [x] Add the `/settings/healthchecks/sources` route in `App.tsx` and a
      `HEALTHCHECK_SOURCES_PATH` helper in `types.ts`
- [x] Update CHK "Source policy" to reflect the dedicated page and the
      confirmation requirement
- [x] Update the `source-reachability` and `source-ingest` e2e specs to navigate
      to the new page and confirm the dialog; add coverage for cancel and for the
      catalog→sources link
- [x] Add a test-cases file
