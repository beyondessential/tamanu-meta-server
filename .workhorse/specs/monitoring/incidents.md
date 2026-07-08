---
id: INC
---

# Incidents

An incident is a span of trouble on a target: a server group, or Canopy as a whole.
It aggregates the issues active on that target over its lifetime, from when it opens until it closes or an operator resolves it.
At most one incident is open per target at a time.

Issues and their severities are defined by the check-state model (see [CHK](checks.md)).
Issues on a server belong to the target of the server's group; issues on an ungrouped server belong to no target and cannot contribute to incidents.
Group-targeted issues belong to that group's target; Canopy-wide issues belong to the Canopy target.

## Membership

An incident opens when an issue at error severity or above becomes active on a target with no open incident.
While an incident is open, every active issue on its target joins it, regardless of severity, so the incident carries the full context of what was wrong during its span.

An issue leaves the incident when it recovers, is resolved, snoozed, or silenced, when its severity drops to debug, or when its server stops being monitored.
The incident closes when its last error-or-worse issue leaves; issues below error severity never hold an incident open.

The membership history — which issues joined and left, and when — is kept and presented as the incident's timeline.
An issue can leave and rejoin the same incident.

Operator actions that change what counts (monitoring toggles, group membership changes, silences, severity reconfiguration) re-evaluate the affected issues' incident membership.

## Notification

Operators are notified over the notification channel: group incidents to the group's configured channel, Canopy-wide incidents to the operator channel.

An incident notifies when it has stayed open past its target's grace period; an incident that closes within grace never notifies.
Whether an incident notified is recorded as its **published** flag, so flaps can be excluded from reporting.
A critical issue joining notifies immediately, bypassing any remaining grace; if the incident has already notified, the critical join escalates it with a further notification, at most once per incident.
A notified incident notifies again when it closes.

## Resolution

An operator can resolve an open incident, recording who and why.
Resolution cascades to the incident's open issues — each is resolved with the same attribution — and the incident closes as its members leave.
Unresolving clears the resolution record; it does not reopen the incident.

Notes attach free-form operator commentary to an incident.
