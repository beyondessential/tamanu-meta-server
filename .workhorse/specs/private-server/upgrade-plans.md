---
id: UPG
---

# Planned upgrades

Canopy records where each deployment is going: the version a group intends to move to, and optionally when.
A plan makes the fleet's intended upgrades visible in one place, and it tells the rest of Canopy which version to hold a deployment's data against, so pre-upgrade testing exercises the version that will actually be applied rather than guessing.

## Scope

This spec covers what a plan records, who sets it, when Canopy considers it met, and what reads it.

It does not cover performing an upgrade.
Canopy serves versions to servers and records what they report running; the act of upgrading is the deployment's own, and a plan is a statement of intent rather than an instruction to anything.

## Why it exists

Canopy already knows what every server runs and every version that exists.
What it cannot derive is where a deployment is going: which minor a deployment moves to next is a decision made by people, weighing what the release contains against what the site can absorb.

Left unrecorded, that decision costs twice.
Pre-upgrade migration testing has to guess, and guesses the newest published version, which is the right default and the wrong answer for a deployment deliberately moving to something older (see [RST](../public-server/restore-replicas.md)).
And nobody can see at a glance which deployments are mid-plan, which are overdue to move, and which have no plan at all.

## A plan

An operator records, per group:

- the **target version**, which must be a published version newer than the group is running;
- an optional **planned date**, the day the upgrade is expected to happen;
- an optional **note**, for whatever an operator needs the next reader to know;
- who recorded it and when.

A group has at most one open plan.
A group moves to one place next, so a second plan replaces the first rather than queueing behind it, and the replaced plan is retained as history.

Plans are managed through the operator interface and are audited.
Deleting a plan says the deployment is no longer going there; it does not say the upgrade happened.

A planned date is a plan, not a deadline.
Canopy neither schedules nor blocks anything on it, and a date that passes changes only how the plan is presented.

## When a plan is met

Canopy decides a plan is met, rather than asking anyone to mark it done.
A plan is met once the group's reported version has reached its target, at which point the plan closes and records when.

Reaching a version *past* the target also meets the plan: a deployment that jumped further than planned has done the upgrade and then some, and holding the plan open would misreport it as outstanding.

A met plan is retained.
The record of what a deployment planned, when it planned it for, and when it actually landed is the fleet's upgrade history, and it is what makes "how long do our upgrades really take to happen" answerable.

## What reads a plan

Pre-upgrade migration testing takes its target from the open plan when a group has one, and falls back to the newest published version when it does not.
A deployment planning to move to an older minor is then tested against that minor, and no restore is spent on a version nobody intends to apply.

A plan changes what is tested, so changing one invalidates nothing already recorded: earlier verdicts stand against the versions they named, and the new target simply becomes the one that has not been tested yet.

## The dashboard

Canopy presents planned upgrades across the fleet in one view, so the question "what is moving, and when" is answered without reading each group.

For each group with an open plan it shows the target version, the version the group is on now, the planned date where there is one, and the pre-upgrade verdict for that target, so an operator sees both the intent and whether the deployment's data survives it.
Groups with no plan are shown too: an unplanned deployment several minors behind is the thing this view exists to surface.

A plan whose date has passed without being met is presented as late.
Late is a presentational state and not an incident: an upgrade slipping is normal operational reality, and Canopy has no basis for treating a date someone typed as a failure of anything.

## Out of scope

- Performing, scheduling, or triggering an upgrade.
- Approving a plan, or gating who may record one beyond the existing operator permissions.
- Planning anything other than a version move, such as an infrastructure migration.
