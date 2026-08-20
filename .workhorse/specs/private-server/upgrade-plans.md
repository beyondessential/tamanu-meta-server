---
id: UPG
---

# Planned upgrades

Canopy records where each deployment is going: the version a group intends to move to, and optionally the day and hour.
A plan makes the fleet's intended upgrades visible in one place, and it tells the rest of Canopy which version to hold a deployment's data against, so pre-upgrade testing exercises the version that will actually be applied rather than guessing.

## Scope

This spec covers what a plan records, who sets it, when Canopy considers it met, and what reads it.

It does not cover performing an upgrade.
Canopy serves versions to servers and records what they report running; the act of upgrading is the deployment's own, and a plan is a statement of intent rather than an instruction to anything.

## Why it exists

Canopy already knows what every server runs and every version that exists.
What it cannot derive is where a deployment is going: which minor a deployment moves to next is a decision made by people, weighing what the release contains against what the site can absorb.

Left unrecorded, that decision costs twice.
Pre-upgrade migration testing has nothing to aim at, so a deployment gets no answer about its own data until it says where it is going (see [RST](../public-server/restore-replicas.md)).
And nobody can see at a glance which deployments are mid-plan, which are overdue to move, and which have no plan at all.

## A plan

An operator records, per group:

- the **target version**, which must be a published version newer than the group is running;
- an optional **planned date**, the day the upgrade is expected to happen;
- an optional **planned time**, the hour on that day the upgrade starts, with the **timezone** it is a wall clock in;
- an optional **note**, for whatever an operator needs the next reader to know;
- who recorded it and when.

An hour needs a day to sit on and a zone to be read in: Canopy holds no timezone for a group, and the fleet spans enough of the world that a bare wall clock means nothing to anyone but whoever typed it.
Most plans have no hour, since which night a deployment moves is usually settled well before what time it starts.

A group has at most one open plan.
A group moves to one place next, so a second plan replaces the first rather than queueing behind it, and the replaced plan is retained as history.

An open plan's date, time, and note can be amended, and an amendment records who made it and when.
A corrected date or a reworded note is the same plan better described, so it stays one plan rather than entering the history as a second.
Changing the target is not an amendment: where a deployment is going is what the history exists to record, so a new target replaces the plan as any other second plan would.
A plan that has been met or replaced is history and is no longer amendable.

Plans are managed through the operator interface and are audited.
Withdrawing a plan says the deployment is no longer going there; it does not say the upgrade happened.
A withdrawn plan is retained and records who withdrew it and when: a deployment that was going somewhere and stopped is part of the same history as one that arrived, and it frees the group to be planned somewhere else.

A planned date is a plan, not a deadline.
Canopy neither schedules nor blocks anything on it, and a date that passes changes only how the plan is presented.

## When a plan is met

Canopy decides a plan is met, rather than asking anyone to mark it done.
A plan is met once the group's reported version has reached its target, at which point the plan closes and records when.

Reaching a version *past* the target also meets the plan: a deployment that jumped further than planned has done the upgrade and then some, and holding the plan open would misreport it as outstanding.

A met plan is retained.
The record of what a deployment planned, when it planned it for, and when it actually landed is the fleet's upgrade history, and it is what makes "how long do our upgrades really take to happen" answerable.

## What reads a plan

Pre-upgrade migration testing takes its target from the open plan, and a group with no plan is not tested at all.
Recording a plan is what asks for the testing, and no restore is spent on a version nobody intends to apply.

A plan changes what is tested, so changing one invalidates nothing already recorded: earlier verdicts stand against the versions they named, and the new target simply becomes the one that has not been tested yet.

## The dashboard

Canopy presents planned upgrades across the fleet in one view, so the question "what is moving, and when" is answered without reading each group.

For each group with an open plan it shows the target version, the version the group is on now, the planned date and hour where there are ones, and the pre-upgrade verdict for that target, so an operator sees both the intent and whether the deployment's data survives it.
The hour carries its zone, abbreviated: whose midnight it is is the whole question a reader has.
Where an attempt is under way it shows that too, since a restore takes hours and a verdict of not-yet-tested otherwise looks the same whether the pipeline is working or has stopped.
Where nothing is declared to migrate the group's data it says so in place of the verdict, since a plan on a deployment with no such declaration is never dispatched and a reader would otherwise wait on a result that cannot arrive.
Groups with no plan are shown too, behind a disclosure that counts them: an unplanned deployment several minors behind is what this view exists to surface, and the count surfaces it without the list crowding out what is moving.

A plan whose date has passed without being met is presented as late, judged on the day alone rather than the hour.
Late is a presentational state and not an incident: an upgrade slipping is normal operational reality, and Canopy has no basis for treating a date someone typed as a failure of anything.

The same view presents the plans that have closed, most recently closed first, so what a deployment planned before is readable beside what it plans now.
Each shows where it was going, the date and hour it was planned for, and how it closed: met, replaced by a later plan, or withdrawn with the operator who withdrew it and when.
A withdrawn plan is otherwise unreadable anywhere, since a deployment that stopped going somewhere leaves no other mark on the fleet.

## The calendar feed

Canopy publishes planned upgrades as an iCalendar feed a calendar application subscribes to, so the fleet's intent appears where people already look to see what a day holds.

The feed is served by the internet-facing interface rather than the operator one, because the calendar services people use fetch a subscription unattended from their own infrastructure and cannot reach the operator network.
Access is carried by a token in the URL: a calendar application has no way to be asked for a credential, so holding the URL is what grants the read.

An operator mints a feed against a label saying whose calendar it is for, and the URL is shown once at minting and never again.
A feed does not expire.
A subscription that lapses stops updating without telling its subscriber, so a feed ends by being revoked, after which the URL serves nothing.
Canopy records when each feed was last fetched, so one nobody subscribed to reads differently from one that is live.

The calendar carries the plans that name a day, while a plan is where the deployment is going and after it has been met.
A replaced or withdrawn plan leaves the calendar, since the deployment is no longer going there; a met one stays as the record of what landed.

An entry names the deployment and the version it is going to, and carries what the plan records beyond the day: the version the deployment is on now, the note, and who planned it, or for a met plan the day it landed.
A plan that names an hour is an entry at that hour, resolved from the zone the plan is a wall clock in so every subscriber reads the same instant whatever their own zone; a plan with only a day is an all-day entry.
An entry marks nobody busy: a planned upgrade is something to know about rather than something that occupies whoever subscribed.

An entry keeps its identity for the life of the plan, so amending a date moves the entry a subscriber already has rather than leaving them holding two.

## Out of scope

- Performing, scheduling, or triggering an upgrade.
- Approving a plan, or gating who may record one beyond the existing operator permissions.
- Planning anything other than a version move, such as an infrastructure migration.
