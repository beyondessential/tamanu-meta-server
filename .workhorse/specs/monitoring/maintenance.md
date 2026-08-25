---
id: MNT
---

# Maintenance windows

A maintenance window is an operator's declaration that a server or a group is being worked on, so what Canopy observes while the work runs raises nothing.
A window is bounded in time, ends itself, and records who declared it and what for, so a quiet part of the fleet is always attributable to a decision someone made.

## Scope

This spec covers declaring a window, what it suspends, how it ends, and how it is presented.
How results are graded is the check-state model (see [CHK](checks.md)), and what a suspended check stops contributing to is the incident spec (see [INC](incidents.md)).

It does not cover planned upgrades (see [UPG](../private-server/upgrade-plans.md)).
A plan records where a deployment is going and when it expects to move; a window declares that work is happening now.

## Why it exists

An upgrade looks exactly like a deployment falling over: the server stops reporting, its checks go stale, and the group opens an incident someone then has to read before recognising it as the work they are already doing.
Alerting through planned work costs an operator the trust they place in the next alert.

Neither existing control answers this.
A silence is one check held down until an operator removes it, so covering an upgrade means writing several and remembering to clear every one afterwards.
Turning a server's monitoring off is per-server and unbounded, and a switch with nothing to turn it back on is one that stays off.

## Declaring

An operator declares a window over one server or one group, giving the time it is expected to end and, optionally, a note saying what is being done.
Canopy records the operator who declared it and when, alongside the expected end and the note.
Declaring, amending, and lifting are administrative actions and are audited (see [ADM](../private-server/admin-access.md)).

A group's window covers the group's own checks and the checks of every server in it, including servers that join the group while it holds.
A server can be covered by its own window and by its group's at the same time, and stays suspended until the last window covering it has ended.

A target has at most one open window.
Declaring over a target that already has one amends that window rather than opening a second, and the amendment records who made it and when.

Canopy never opens a window by itself.
A group with an open upgrade plan is offered the declaration from that plan, prefilled with the plan's window and note, so declaring is one action at the moment the work starts.
The plan supplies the prefill and triggers nothing: an hour someone typed in advance is not evidence that work began.

## What a window suspends

While a window holds over a target, every check on that target has an effective result of skipped, whatever it was observed as (see [CHK](checks.md), "Policy").
It is the transform a silence applies, applied to the target's every check for as long as the window holds rather than to one check until an operator intervenes.

Observed results continue to be recorded throughout, so what a server said during the work is readable afterwards, and a window never costs Canopy the record of what happened while it held.
A window changes how Canopy grades what it observes and nothing else: sources are expected to report as they always were, and are told to run their checks as they always were.

A skipped check is not an issue, so a target under a window contributes nothing to incidents and raises no notification.
An issue that was part of an open incident leaves it when the window is declared.
An incident whose last effective failure leaves this way closes immediately, as it does for any other operator action (see [INC](incidents.md), "Membership").
Where that close is notified, the notice says the incident closed because maintenance was declared, so a reader does not take it as the problem having gone away.

Canopy-wide checks are Canopy monitoring its own operation and are never suspended by a window over a server or a group (see [SELF](../private-server/self-alerts.md)).

## Ending

A window ends when its expected end passes, or when an operator lifts it earlier.
Canopy ends a window at its expected end without asking anyone, so a window left forgotten does not leave a deployment unwatched indefinitely.
An operator whose work is running long extends the window by amending its end before it passes; once a window has ended it is history, and resuming suspension is a fresh declaration.

Canopy records how each window ended: the expected end passing, or the operator who lifted it and when.
Ended windows are retained as the target's maintenance history, so what was being done the last time a deployment went quiet is readable against it.

## Settling

A window's suspension persists for a settle period after the window ends.

A server is back before the sources on it have reported again, and a server whose every source is stale is unreachable (see [CHK](checks.md), "Reachability").
Ending suspension at the instant the work finishes would therefore report as failed the deployment that has just come back, for as long as the work took.
Suspension outlasting the window by a settle period gives the reporters on a server time to be heard from before Canopy judges them.

Checks are suspended and observed results are recorded during settling exactly as during the window itself.
When the settle period elapses, every check on the target is graded normally again, and anything still degraded contributes from that point as it would have all along.
The settle period is the same for every window.

## Notification

Canopy notifies operators over the target's notification channel when a window is declared and again when its suspension ends (see [INC](incidents.md), "Notification").
The declaration names the target, the operator, the expected end, and the note.
The ending names the target, says whether an operator lifted the window or its expected end passed, and says when watching resumes.
A target with no notification channel notifies nowhere.

## Presentation

A target under a window is presented as under maintenance wherever its health or reachability is presented as it currently stands, in the manner an unmonitored server is marked (see [CHK](checks.md), "Monitoring gate").
Its health is muted and carries an indicator naming the window and when it ends, so a failing server under maintenance is never read as a failing server nobody has noticed, and never read as an unmonitored one either.
The status legend names the mark.

Canopy presents every open window across the fleet in one view: what each covers, who declared it, when it ends, and its note.
The view answers "what are we not watching right now" without reading each group.

A target's own surface presents its open window with the actions to amend or lift it, and presents its ended windows as history.
A check whose effective result is skipped because a window holds says so where its result is presented, rather than leaving the skip unexplained.

## Out of scope

- Performing, scheduling, or triggering the maintenance itself.
- Declaring a window to begin at a future time. A window says work is happening now; an intended window is what a plan records (see [UPG](../private-server/upgrade-plans.md)).
- Suspending Canopy's own self-monitoring.
- Exempting a check from suspension so it still alerts while a window holds.
