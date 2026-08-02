# Redacting managed restore replicas

Scenarios verifying that Canopy decides what a redacting replica is masked
with, that a declaration's flag is the only thing that can turn redaction on,
that a replica which cannot be masked is withheld rather than served in the
clear, and that a redaction which only partly applied is visible as its own
finding without disturbing restore health.

## The masking manifest

- [x] The URL a Tamanu version resolves to matches where the product actually publishes its manifests — verifies spec: RST
- [x] Only a product that publishes masking manifests can have a redacting replica — verifies spec: RST
- [x] A redacting declaration's worklist entry carries the product's manifest template, version query, and base-version fallback — verifies spec: RST
- [x] A server whose product publishes no manifest contributes no worklist entry, while the rest of the group's servers still do — verifies spec: RST

## Canopy owns the masking parameters

- [x] A declaration that does not redact sends the manifest URL unset, even when a value is stored against it — verifies spec: RST
- [x] An operator's value for a masking parameter is dropped rather than stored, while the intent's other parameters are kept — verifies spec: RST
- [x] An intent that does not carry the `redact` semantic refuses the flag rather than storing an intent it cannot honour — verifies spec: RST
- [x] The declare and edit forms offer the redaction switch and no manifest fields, for an intent that can redact — verifies spec: RST
- [x] Toggling a declaration's enabled state from the list leaves its redaction flag alone — verifies spec: RST

## Reporting and alerting

- [x] A partial redaction is recorded with its column counts and raises the redaction check, while restore health stays untouched — verifies spec: RST
- [x] A failed redaction raises the check, and a later report that fully applies recovers it — verifies spec: RST
- [x] A report carrying no redaction files no redaction check — verifies spec: RST
- [x] A declaration's row shows what its replicas actually got, so a partial redaction reads as partial from the list — verifies spec: RST
- [x] The masked and skipped column counts and the manifest version are readable without opening the raw health data — verifies spec: RST

## Corroboration against published manifests

- [x] A redacting declaration names the servers it covers that have no masking, and why — verifies spec: RST
- [x] A server whose reported version published no manifest is named as a gap, while one whose version did is not — verifies spec: RST
- [x] A server Canopy holds no version for is not named, since the consumer resolves the manifest against the data it restored — verifies spec: RST
