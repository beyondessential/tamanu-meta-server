---
id: INC
---

# Incidents

An incident is a span of trouble on a target: a server group, or Canopy as a whole.
It aggregates the issues active on that target over its lifetime, from when it opens until it closes or an operator resolves it.
At most one incident is open per target at a time.

Issues and effective results are defined by the check-state model (see [CHK](checks.md)).
Issues on a server belong to the target of the server's group; issues on an ungrouped server belong to no target and cannot contribute to incidents.
Group-targeted issues belong to that group's target; Canopy-wide issues belong to the Canopy target.

## Membership

An incident opens when a check's effective result becomes failed on a target with no open incident.
While an incident is open, every issue on its target joins it — effective warnings included — so the incident carries the full context of what was wrong during its span.

An issue leaves the incident when it stops being one: its effective result recovers (to passed or skipped, whether by report or by policy), it is resolved or snoozed, or its server stops being monitored.
Warnings never hold an incident open.

When the last effective failure leaves because its result recovered, the incident does not close immediately: it **lingers** for its target's linger window, remaining the target's open incident.
A check whose effective result becomes failed during the window — a fresh failure or the same one returning — ends the lingering and the incident continues.
An incident whose linger window elapses without an effective failure closes, recording the close as of when its last effective failure left.
Lingering damps reporter flapping, not operator action: a last failure leaving through resolution, snooze, silence, or its server's monitoring being turned off closes the incident immediately.

The membership history — which issues joined and left, and when — is kept and presented as the incident's timeline.
An issue can leave and rejoin the same incident.

Operator actions that change what counts (monitoring toggles, group membership changes, policy and silence changes) re-evaluate the affected issues' incident membership.

Membership evaluation is asynchronous. A report records its issue state immediately; the resulting open, join, leave, or close follows within a short bounded delay rather than synchronously with the report. Membership is therefore eventually consistent with the current issue state.

## Notification

Operators are notified over the notification channel: group incidents to the group's configured channel, Canopy-wide incidents to the operator channel.

An incident notifies when it has stayed open past its target's grace period; the notification additionally waits out any lingering, so it is sent only while an effective failure is live.
An incident that closes before its notification was sent never notifies.
Whether an incident notified is recorded as its **published** flag, so flaps can be excluded from reporting.
An escalating check's effective failure (see [CHK](checks.md), "Policy") notifies immediately, bypassing any remaining grace; if the incident has already notified, the join escalates it with a further notification, at most once per incident.
A notified incident notifies again when it closes.

## Resolution

An operator can resolve an open incident, recording who and why.
Resolution cascades to the incident's open issues — each is resolved with the same attribution — and the incident closes as its members leave.
Unresolving clears the resolution record; it does not reopen the incident.

Notes attach free-form operator commentary to an incident.

## Manual incidents

Alongside the automatic incidents above, Canopy keeps manual incidents: records of incidents the support team managed, written after the fact rather than derived from check state.

A manual incident carries a title, a markdown description, when it started, and when it ended; an ended time may be absent while the incident is ongoing.
Every manual incident names exactly one server group as the affected target, and a group with manual incidents on record cannot be removed: the record is history.
Each records who created it and when it was created and last changed.

Manual incidents are independent of the check-state model: no issue joins them, they never notify, and nothing opens, closes, or resolves them except the people editing them.
They are created, edited, and deleted both over the MCP interface (see [MCP](../private-server/mcp.md)) and in the operator UI, where they are presented alongside automatic incidents; either way every write is attributed to the identity that made it.
