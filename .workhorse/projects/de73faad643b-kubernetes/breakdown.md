# Automate Everything — card breakdown

The first third of the project: the one remaining gate plus the work that has no upstream dependency. Migration testing is complete and in QA, so it is not represented here; the cards it unblocks downstream (the Ansible chain, RC migration testing's dependants) belong to later thirds.

## Reporting-schema discovery

Discovery with the analytics team, who own Maui and the DBT system, into how reporting schemas work today: how they are created and by whom, how they are tested and against what replica, what the handover to the deployment team looks like, and where the DBT run lives, what it needs, and what it costs to run. The output decides which shape the reporting-schema pipeline takes — fully in Canopy or managed from Canopy — and it may differ per stage. This is calendar-bound on the analytics team rather than effort-bound, so it starts immediately even though it delivers late; if it slips, the whole reporting half of the project slips with it.

## Migration testing in the RC cycle

Run migration testing as part of the release-candidate (regression-testing candidate) cycle and track the results in Canopy, so a candidate's effect on real deployment data is known before it becomes a release. Builds directly on the completed migration-testing loop and its managed-restore machinery. The work lands in the Tamanu repo.

## Settle deployment, group, and rank terminology

Canopy's language for a deployment is inconsistent: `servers/products.md` takes a server's deployment from its group (the project-manager sense), while VitiOps means a single rank within a group. Pick the sense Canopy uses, name the others, and make the UI, specs, and code agree. No dependencies, and it clears the ground for the inventory work by settling the group-plus-rank addressing that inventory state is held against.

## Canopy holds inventory state

Canopy holds the desired version per group and rank, alongside the current version it already learns from health checks, so drift between intended and actual becomes visible rather than implicit. More broadly, this is the Ansible inventory state for deployments with Linux servers, of which the Tamanu version is one field; how much of that inventory Canopy holds, and in what form, is settled during shaping. This is the first state Canopy authors rather than observes, and it gates the downstream Ansible chain. Inventory state also needs adding to the recovery escrow, since once the file system stops being the source of truth there is no second copy.

## Audit the release and RC process

A paper exercise with no dependencies: audit the current release process end to end and enumerate what is still manual and could be automated, working toward everything except the release trigger itself being automated. Confirm whether creating a release issue is already automated before treating it as done.

## Canopy quick-wins sweep and bug audit

Sweep the Canopy Workhorse board for existing small issues that reduce friction in automated workflows and pull the relevant ones into this project. Check whether the earlier Canopy bug audit was completed and finish it off if not. Fill-in work that runs alongside the rest rather than waiting on it.
