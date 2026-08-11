# VitiOps deployment automation

## Overview

VitiOps runs the actual deployments of Tamanu and its related software onto production servers, plus the pre-production work around them — cloning, upgrade testing, release candidates.
Too much of that is still hand-driven: a migration is proven by someone restoring a database and running it, a reporting schema arrives as a SQL file handed between teams, and a release is a checklist of steps that mostly could run themselves.
This project moves that burden into Canopy, so the fleet's deployment work is commissioned, tracked, and reported on automatically, and a human only does the parts that genuinely need a human decision.

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

Not necessarily Canopy code; likely bestool (where the artefact is consumed) or Seedling.

- Apply a reporting artefact directly to the deployment's database, with no manual SQL-running step
- Report application back to Canopy so the deployment's current reporting-schema state is known, not assumed
- Which repo this lands in is an open question, but it's in this project's scope either way

## Release and RC process

Goal: everything except the release trigger itself is automated. The trigger stays manual because quality approval is a human call.

- Audit the current release process end to end and enumerate what is still manual and could be automated. Creating a release issue is believed to already be automated away — confirm before treating it as done
- **Migration testing in the RC** — run migration testing as part of the release candidate (regression-testing candidate) cycle, and track the results in Canopy, so a candidate's effect on real deployment data is known before it's a release
- **Seed snapshots** — a separate Seedling concept living in the Tamanu repo. Relationship to Canopy's migration testing needs to be worked out: whether Canopy tracks them, uses them, or leaves them alone

## Canopy smoothing

Smaller work that makes the automation above tolerable to operate.

- Sweep the Canopy Workhorse board for existing small issues that reduce friction in automated workflows, and pull the relevant ones into this project
- **Bug audit** — a Canopy bug audit was carried out; check whether it was completed and finish it off if not

## Open questions

- Which of "fully in Canopy" and "managed from Canopy" the reporting-schema pipeline takes — blocked on discovery with analytics and Maui
- Where artefact application lands: bestool, Seedling, or both
- Whether seed snapshots become a Canopy-tracked concept or stay entirely in the Tamanu repo
- Whether the release-issue automation is already complete
