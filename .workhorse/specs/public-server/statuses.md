---
id: STA
---

# Status reporting

Devices report on their server by pushing statuses to the device API.
A status is one source's complete current picture of the server: its health checks and their results, plus server-wide metadata.
What Canopy does with the checks is the check-state model (see [CHK](../monitoring/checks.md)).

## Push

A status is pushed to the server it describes; the caller must present the server's enrolled device certificate, or a certificate holding the admin role.

The payload carries:

- **source** — the name of the reporter pushing this status.
  Transitionally optional: a push without a source is attributed to `alertd`.
  The field will become mandatory; new reporters must send it.
  The reserved source names (see [CHK](../monitoring/checks.md), "Sources") are rejected.
  The `alertd` source is also populated by Canopy itself for Kubernetes servers it harvests, so a device push is one of two origins for it (see [K8S](../monitoring/kubernetes.md)).
- **health** — the source's complete set of checks: for each, the check's name, exactly one result (`passed`, `warning`, `failed`, `broken`, or `skipped`), and any further detail fields, which are recorded verbatim against the check.
  The set may be empty, meaning the source currently has no checks — which recovers every check it previously reported.
- any further top-level fields, recorded verbatim as the status's server-wide detail.

The server's application version is taken from the server-wide detail, falling back to the pushing client's version header.

Per the check-state model, the push is the source's whole truth: checks omitted from it are recovered, and other sources' checks are unaffected.

The source's ingest mode (see [CHK](../monitoring/checks.md), "Source policy") gates the push: an `allow` source is ingested as above; an `ignore` source's push is accepted but recorded nowhere, its checks and detail discarded; a `deny` source's push is rejected. Gating is per source, so other sources on the same server are unaffected.

Every ingested push is recorded in full as the server's status history, held as described by the history-storage rules (see [HST](../platform/history-storage.md)).

## Legacy pushes

A push without a health field is a legacy report from a Tamanu server.
It is recorded with its metadata like any other status, and is treated as the source `tamanu` reporting a single check `tasks` as passed — a liveness heartbeat that participates in source staleness like any source.

## Response

The response to a push carries only what the pushing source needs; a source is sent nothing meant for another source, and relies on receiving nothing beyond its own concerns.

- Each check in the push is answered with the policy applied to it (see [CHK](../monitoring/checks.md), "Policy"), so a source sees how its reports are graded and can stop running checks whose policy is `skipped`.
- Whether the server should start a backup now is returned only to the source that runs backups (`alertd`).
- The server's effective tags, a server-wide fact, are returned to every source.
- The names the server is entitled to act on, likewise a server-wide fact, are returned to every source, so an agent learns of a new domain or a newly granted permission from a push it was making anyway (see [CRT](certificates.md), "What a server may act on").
