# Automate Everything

## Overview

VitiOps runs the actual deployments of Tamanu and its related software onto production servers, plus the pre-production work around them — cloning, upgrade testing, release candidates.
Too much of that is still hand-driven: a migration is proven by someone restoring a database and running it, a reporting schema arrives as a SQL file handed between teams, and a release is a checklist of steps that mostly could run themselves.
This project moves that burden into Canopy, so the fleet's deployment work is commissioned, tracked, and reported on automatically, and a human only does the parts that genuinely need a human decision.

## Sequencing

Two things gate everything else, and neither blocks the other, so both start now.

**Migration testing is the critical path.** It is the only component with code already in the ground, and proving it end to end also proves the managed-restore machinery that the reporting-schema pipeline reuses. Nothing downstream is worth building against an unproven replica.

**Discovery is blocked on people, not code.** Analytics and Maui have to be in the room, which makes it calendar-bound rather than effort-bound. It starts immediately even though it delivers late; if it slips, the entire reporting half of the project slips with it.

The rest falls out of those two:

- **Now, unblocked** — migration testing end to end; reporting-schema discovery; the release-process audit, which is a paper exercise with no dependencies; quick wins
- **Once migration testing is proven** — migration testing in the RC cycle
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

## Reporting schemas

Reporting schemas are produced by a DBT-based system, run against a replica of the production database for testing and finalisation, and emitted as a SQL file that the deployment team applies to that deployment's production servers.

### Discovery

Precondition for the rest of this component — needs the analytics team and the Maui team.

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
- **Seed snapshots** stay a Tamanu concern. Canopy does not track or consume them, and migration testing runs against restored deployment data rather than a seed snapshot, which is the point of it. Out of scope for this project

## Quick wins

The deliverable here is Canopy itself working better. Everything above lands in Canopy and is operated through it, so friction in Canopy is friction in the whole project. This is fill-in work that runs alongside the rest rather than waiting on it.

- Sweep the Canopy Workhorse board for existing small issues that reduce friction in automated workflows, and pull the relevant ones into this project
- **Bug audit** — a Canopy bug audit was carried out; check whether it was completed and finish it off if not

## Open questions

Answerable by someone who already knows, and worth asking before this project shapes any cards:

- Whether the release-issue automation is already complete

Needs real work to answer:

- Which of "fully in Canopy" and "managed from Canopy" the reporting-schema pipeline takes — blocked on discovery with analytics and Maui
- How much of migration testing is actually left, which the spec-against-code audit answers and nothing else will