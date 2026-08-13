# Automate Everything

## Overview

VitiOps runs the actual deployments of Tamanu and its related software onto production servers, plus the pre-production work around them — cloning, upgrade testing, release candidates.
Too much of that is still hand-driven: a migration is proven by someone restoring a database and running it, a reporting schema arrives as a SQL file handed between teams, and a release is a checklist of steps that mostly could run themselves.
This project moves that burden into Canopy, so the fleet's deployment work is commissioned, tracked, and reported on automatically, and a human only does the parts that genuinely need a human decision.

**This is a multi-repo project.** Canopy is where the work is commissioned, tracked, and reported, but the code lands wherever the job is done: Canopy, bestool, Seedling, Tamanu, and ops. Cards are shaped here and move to their respective workspace when work starts on them. A component being someone else's repo does not put it outside the project.

## QA
- Need to 

## Sequencing

Two things gate everything else, and neither blocks the other, so both start now.

**Migration testing is the critical path.** It is the only component with code already in the ground, and proving it end to end also proves the managed-restore machinery that the reporting-schema pipeline reuses. Nothing downstream is worth building against an unproven replica.

**Discovery is blocked on people, not code.** It needs the analytics team's time, which makes it calendar-bound rather than effort-bound. It starts immediately even though it delivers late; if it slips, the entire reporting half of the project slips with it.

The rest falls out of those two:

- **Now, unblocked** — migration testing end to end; reporting-schema discovery; Canopy holding desired versions and inventory state; the release-process audit, which is a paper exercise with no dependencies; quick wins
- **Once migration testing is proven** — migration testing in the RC cycle
- **Once Canopy holds inventory state** — the Ansible plugin that makes Canopy the inventory source, then the refusal policies that ride on it
- **Once discovery lands** — deployment artefacts, whose shape depends on what the pipeline produces and who produces it, then the reporting-schema pipeline itself
- **Last** — applying artefacts, which needs artefacts to exist and the bestool-or-Seedling question answered

## Migration testing

Half-built in Canopy today: candidate versions, dispatch, and report shapes exist ([RST](../../specs/public-server/restore-replicas.md) *Pre-upgrade migration testing*, `crates/database/src/migration_tests.rs`, `crates/private-server/src/fns/migration_tests.rs`).
Finish it and prove it end to end.

- The full loop, exercised for real: take a backup → restore it through the Canopy managed-restore process → apply the target version's Tamanu migrations against that data → report the outcome back to Canopy
- Close whatever gaps remain between the spec and the code — audit spec against implementation before shaping cards
- Prove it against a real deployment's data, not a synthetic database. The whole point of the feature is that a migration's behaviour is a property of *that deployment's* data
- Reporting: verdict, target version, which migration failed and its error, and how long the chain took — the duration matters as much as the pass/fail, because a migration that succeeds but overruns the upgrade window is still a blocker
- Operator-facing surface: where a VitiOps person looks to see whether a version is safe for a deployment before scheduling its window
- Detailed shape of the remaining gaps can be filled in during card shaping, once the spec/code audit is done

## Desired versions and Ansible inventory

The ops repo's Ansible configuration is the source of truth for deploying Linux servers. Because it is a git repo, it drifts: with this many deployments and this many people working at once, changes get made locally and never pushed or raised as a PR. This is routine, not exceptional.

Two consequences, both of which have bitten:

- **Incident response against stale truth.** A DevOps engineer responding to an incident reads an older Ansible configuration than the one actually applied to the server, and works from it
- **Misapplied upgrades.** A configuration edited locally for an experiment and not reverted before a playbook run has upgraded deployments that were not scheduled for it. No deployment has been fully upgraded by accident, but the first half of an upgrade has been applied and then reverted. The only control is a local file, so this is easy to fall into and we remain permissive about it

### What Canopy holds

Canopy already learns each server's *current* Tamanu version through health checks, and that stays the truth of what is on the server. The gap is the *desired* version: what we intend that deployment to be on. Holding both makes drift visible rather than implicit.

- The desired version per deployment, sitting alongside migration testing and the intent-to-upgrade plans this project already builds on
- More broadly, the Ansible inventory state for deployments with Linux servers, of which the Tamanu version is one field

### Deployment and rank

Terminology needs care here, because "deployment" means different things to different audiences:

- Canopy models a **server group**, holding all of a deployment's servers across every rank, and a **rank** on each server (production, clone, demo, test, dev)
- To VitiOps and the deployment team, a **deployment** is usually one rank within a group: production and demo in the same group are two deployments
- To project managers, all of those servers are one deployment
- Canopy addresses this as group plus rank, and inventory state is held per group and rank, matching the VitiOps sense

Canopy's own language is not yet consistent about this, and the inventory work is where it starts to bite. Settling it is a quick-win card, below.

### Canopy as a control point

Once the plugin pulls inventory from Canopy, Canopy can decline to serve it. That is the whole mechanism: nothing has to wrap or intercept the Ansible run itself, which is the expensive way to get the same control. The plugin asks, and Canopy says no.

Two things it buys:

- **Concurrency.** On every run, not just privileged ones. Someone else is already mid-upgrade on this deployment, or changing its settings, so this run is refused and told who to talk to. Today nothing knows that another person is working on the same deployment
- **Authorisation.** Whether this person may do production maintenance or a production upgrade at all, expressed as an unlock inside Canopy

Concurrency is the more valuable of the two and the less contentious, since it blocks collisions rather than people.

### Durability

This is the first state Canopy *authors* rather than observes. Everything else it holds is reported by devices or derived from what they report, so it can be relearnt by asking the fleet again. Once the file system stops being the source of truth, the inventory has no second copy.

That puts it squarely in the recovery escrow ([BKJ](../../specs/jobs/backup.md) *Recovery escrow*), which already carries the group, server, configuration, schedule, and capability records needed to recover without Canopy. Inventory state, desired versions included, is the same class of thing: what you need to rebuild a deployment when Canopy is gone.

### Cards

- Canopy holds the inventory state, including the desired version, per group and rank
- Inventory state is added to the recovery escrow
- **Separately**, an Ansible plugin, or whatever mechanism fits, that makes Canopy the inventory source instead of the file system
- Canopy refusing to serve inventory, with the concurrency check as the first policy on it
- The production maintenance and upgrade unlock, on the same mechanism
- iCal feed for upgrades

Longer term this is the foundation for Canopy controlling upgrades directly, which is not this project.

### Side benefit: a public Ansible repo

Worth calling out even if no work happens on it this cycle. Once inventory state lives in Canopy, no private information remains in the Ansible configuration. The same was done separately for the Pulumi configuration. That clears the way to make the Ansible repo public on GitHub, which we have been asked to do before and which carries potential financial benefits. An external incentive stacked on top of the in-scope ones.

## Reporting schemas

Reporting schemas are produced by a DBT-based system, run against a replica of the production database for testing and finalisation, and emitted as a SQL file that the deployment team applies to that deployment's production servers.

### Discovery

Precondition for the rest of this component — needs the analytics team, who own Maui and the DBT system.

- How reporting schemas are currently created, and by whom
- How they are tested, and against what replica
- What the handover to the deployment team actually looks like today
- Where the DBT run lives, what it needs, and what it costs to run

### Bringing it into Canopy

Two candidate shapes; discovery decides which, and it may differ per stage.

- **Fully in Canopy** — Canopy runs the DBT pipeline itself against a managed restore replica
- **Managed from Canopy** — Canopy commissions the run from the Maui/DBT system, which generates the SQL file and returns it; Canopy takes delivery of the artefact
- Either way, Canopy already holds the replica authority ([RST](../../specs/public-server/restore-replicas.md)) — a reporting-schema run wants a replica of that deployment's data, which is the same thing migration testing needs
- Canopy is where the resulting artefact lands, is versioned, and is associated with the deployment it was built for

## Deployment artefacts

A new concept: artefacts that are specific to a single deployment, rather than published fleet-wide.

- A reporting-schema SQL file is the first instance; the mechanism should not be reporting-specific
- **Privacy** — there is no real risk in the artefact's contents being seen, but a fleet-wide artefact listing would expose the client list. Deployment-specific artefacts are distinguished from published ones and are not enumerable by parties who shouldn't see who our deployments are
- Versioning and provenance: which run produced this artefact, from which snapshot, for which deployment
- Detailed shape — storage, naming, access control — can be filled in during card shaping

## Applying artefacts

Not Canopy code. This lands in **both bestool and Seedling**: the transition to Seedling is long, and bestool stays the consumer for the whole of it, so a bestool-only implementation strands every deployment that has not moved yet and a Seedling-only one serves nobody today.

- Apply a reporting artefact directly to the deployment's database, with no manual SQL-running step
- Report application back to Canopy so the deployment's current reporting-schema state is known, not assumed
- **Two cards, one per repo**, implementing the same behaviour against different consumers. Both are shaped here and move to their respective workspace when work starts on them

## Release and RC process

Goal: everything except the release trigger itself is automated. The trigger stays manual because quality approval is a human call.

- Audit the current release process end to end and enumerate what is still manual and could be automated. Creating a release issue is believed to already be automated away — confirm before treating it as done
- **Migration testing in the RC** — run migration testing as part of the release candidate (regression-testing candidate) cycle, and track the results in Canopy, so a candidate's effect on real deployment data is known before it's a release
- **Seed snapshots** remain a Tamanu-repo concept and are not pulled into Canopy: Canopy does not track or consume them, and migration testing runs against restored deployment data rather than a seed snapshot, which is the point of it. In scope for this project, with the work landing in the Tamanu repo

## Quick wins

The deliverable here is Canopy itself working better. Everything above lands in Canopy and is operated through it, so friction in Canopy is friction in the whole project. This is fill-in work that runs alongside the rest rather than waiting on it.

- Sweep the Canopy Workhorse board for existing small issues that reduce friction in automated workflows, and pull the relevant ones into this project
- **Bug audit** — a Canopy bug audit was carried out; check whether it was completed and finish it off if not
- **Settle what a deployment, group, and rank each mean**, then make the UI, specs, and code agree. Today they do not: `servers/products.md` takes a server's deployment from its group, which is the project-manager sense, while VitiOps means a single rank within a group. Pick the sense Canopy uses, name the other one, and apply it consistently. No dependencies, and it clears the ground for the inventory work above

## Open questions

Answerable by someone who already knows, and worth asking before this project shapes any cards:

- Whether the release-issue automation is already complete

Needs real work to answer:

- Which of "fully in Canopy" and "managed from Canopy" the reporting-schema pipeline takes — blocked on discovery with the analytics team
- How much of migration testing is actually left, which the spec-against-code audit answers and nothing else will
- How much of the Ansible inventory Canopy holds, and in what form: the desired version alone, or the full inventory state for Linux deployments