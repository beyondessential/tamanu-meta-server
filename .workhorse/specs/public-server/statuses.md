---
id: STA
---

# Status reporting

Agents report by pushing statuses to the device API.
A status is one source's complete current picture of a machine and the applications on it: their health checks and their results, plus the detail each carries.
What Canopy does with the checks is the check-state model (see [CHK](../monitoring/checks.md)).

## Push

A status is pushed to the machine it describes; the caller must present that machine's enrolled identity, or an identity holding the admin role.

The payload carries:

- **source** — the name of the reporter pushing this status.
  Transitionally optional: a push without a source is attributed to `alertd`.
  The field will become mandatory; new reporters must send it.
  The reserved source names (see [CHK](../monitoring/checks.md), "Sources") are rejected.
- **machine** — the machine's health checks and its detail.
- **applications** — the applications the reporter found, each with its own health checks and detail.

Per the check-state model, the push is the source's whole truth for each target it describes: checks omitted from it are recovered, and other sources' checks are unaffected.

The source's ingest mode (see [CHK](../monitoring/checks.md), "Source policy") gates the push: an `allow` source is ingested as above; an `ignore` source's push is accepted but recorded nowhere, its checks and detail discarded; a `deny` source's push is rejected.
Gating is per source, so other sources on the same machine are unaffected.

Every ingested push is recorded in full as the machine's status history, held as described by the history-storage rules (see [HST](../platform/history-storage.md)).

## Health and detail

Wherever the payload describes a machine or an application, it does so the same way: a set of health checks, and a `detail` object.

A health check carries its name, exactly one result (`passed`, `warning`, `failed`, `broken`, or `skipped`), and its own `detail` object.
The set may be empty, meaning the source currently has no checks for that target — which recovers every check it previously reported for it.

Everything a reporter has to say beyond the structure sits inside a `detail` object, and nothing is spread across the envelope.
A reporter can therefore report a field of any name, including one the envelope itself uses, and Canopy can gain a sibling of `health` and `detail` later without ambiguity about what an unrecognised key meant.

Detail is recorded verbatim against the check or target it was attached to.
Policy rules and the fleet spread reach a check's fields as `check.<field>`, matching where they sit in the payload.

A check's name is reported bare.
Canopy qualifies an application's checks with that application's type when cataloguing them (see [CHK](../monitoring/checks.md), "Names").

## Identifying an application

A reporter cannot know what Canopy calls the applications on a machine, and on a machine's first report there is nothing to know.
So a report describes what the reporter found, and Canopy decides what that corresponds to.

Each application in a push is identified by a **key** the reporter chooses, and carries the application's **type**.
The key must be unique among the applications on that machine and must identify the same application across that reporter's pushes.
What a reporter derives its keys from is the reporter's own business.

`applications` is an object keyed by that key, so a payload cannot express two applications sharing one.

Canopy correlates a reported application to its own record by the machine, the key and the type together, and never discloses its own identifier to the reporter.
Because the type is part of that correlation, a reporter that reports a different type under a key it was already using has stopped reporting one application and started reporting another, and Canopy treats it as exactly that.

An application in a push that Canopy does not already hold is created (see [FLT](../servers/overview.md), "Applications come from reports").

## Transitional unified pushes

Canopy accepts an earlier format in which a machine's and its application's material are not separated: one set of health checks and one flat body of detail fields, describing a host assumed to run a single application.

A push carrying a `machine` section is in the current format and is taken as given.
A push carrying health checks but no `machine` section is a unified push, and Canopy separates it into machine-subject and application-subject material itself before ingesting it.
A push carrying no health field at all is a legacy Tamanu report.
An empty set of health checks is a different thing: the reporter is describing the target and saying it currently has no checks for it, so the push is ingested as any other and recovers what that source previously reported.

Canopy holds the list of check names and detail fields that are machine-subject in order to do that separation.
Everything not on that list is application-subject, so a check or field Canopy does not recognise is filed against the application, which is where an unrecognised one has always gone.

A unified push describes at most one application, the format having no way to say otherwise, and Canopy works out which one from the push itself.

The only thing a unified push says about what its reporter is, is the role its Tamanu application plays.
Where it says that, Canopy correlates on the type that role and its software make, and adopts an application of that type the machine does not already hold.
Where it does not, the machine's own record answers, which it can as long as it holds exactly one application.

A machine holding no application at all is a real case rather than a failure, because the fleet holds boxes running nothing Canopy models and their reporters push the same shape as everyone else.
Such a push is the machine's in full: every check in it files against the machine, every check is identified in the machine's namespace whatever its name, and all of its detail is recorded as machine detail.

A push that names no application Canopy holds for a machine that holds several is refused.
Attributing a machine's whole picture to an arbitrary one of the applications on it is the thing separating the two grains exists to prevent.

## Legacy pushes

A push without a health field is a legacy report from a Tamanu application.
It is recorded with its detail like any other status, and is treated as the source `tamanu` reporting a single check `tasks` as passed — a liveness heartbeat that participates in source staleness like any source.

## Response

The response to a push carries only what the pushing source needs; a source is sent nothing meant for another source, and relies on receiving nothing beyond its own concerns.

- Each check in the push is answered with the policy applied to it (see [CHK](../monitoring/checks.md), "Policy"), so a source sees how its reports are graded and can stop running checks whose policy is `skipped`.
- Whether a backup should start now is returned only to the source that runs backups (`alertd`).
- The effective tags of the machine and of each application described are returned to every source, so an agent can read the classification Canopy holds for what it reports on.
- The names each application on the machine is entitled to act on are likewise returned to every source, so an agent learns of a new domain or a newly granted permission from a push it was making anyway (see [CRT](certificates.md), "What an application may act on").
