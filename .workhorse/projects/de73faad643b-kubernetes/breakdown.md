# Automate Everything — card breakdown

The cards are listed in dependency order. The first six are the first third: the one remaining gate plus the work that has no upstream dependency. The four that follow are the second third, each riding on a first-third card landing — the Ansible chain on inventory state, deployment artefacts on discovery. Migration testing is complete and in QA, so it is not represented here. The final third — the upgrade unlock, the reporting-schema pipeline, applying artefacts in bestool and Seedling, seed snapshots, and ongoing QA — is not yet staged.

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

## Ansible plugin making Canopy the inventory source

An Ansible plugin, or whatever mechanism fits, that pulls inventory from Canopy instead of the file system, so Canopy becomes the source of truth for deploying Linux servers rather than a git repo that drifts. Rides on Canopy holding inventory state. This is the change that makes the file system stop being the source of truth, and the precondition for Canopy being able to decline to serve a run at all. It is also what clears the private information out of the Ansible configuration, opening the separate possibility of making the Ansible repo public.

## Canopy refusing to serve inventory, with concurrency as the first policy

Once the plugin asks Canopy for inventory, Canopy can decline to serve it — nothing has to wrap or intercept the Ansible run itself. The first policy on that mechanism is concurrency: a run is refused when someone else is already mid-upgrade on the deployment or changing its settings, and told who to talk to. Concurrency is the more valuable and less contentious control because it blocks collisions rather than people. Rides on the plugin being the inventory source.

## Deployment artefacts

A new concept: artefacts specific to a single deployment rather than published fleet-wide, with a reporting-schema SQL file as the first instance but the mechanism not reporting-specific. Deployment-specific artefacts are distinguished from published ones and are not enumerable by parties who should not see the client list, and each carries versioning and provenance — which run produced it, from which snapshot, for which deployment. Storage, naming, and access control are settled during shaping. Rides on discovery, since the artefact's shape depends on what the pipeline produces and who produces it.

## iCal feed for upgrades

An iCal feed of scheduled upgrades, published from the inventory and upgrade-intent state Canopy now holds. Rides on Canopy holding inventory state.