---
id: ART
---

# Version artefacts

An artefact is a file published for a version that Canopy records the location of and does not hold: an installer, a package, a set of migrations, a manifest.
Canopy is the index rather than the store: a server or the infrastructure acting on a version's behalf learns from Canopy where a file is and fetches it from there, and where the location is not one that caller may read, Canopy passes the bytes through on request rather than keeping a copy.

## What an artefact belongs to

An artefact belongs to one exact version, or to a range of versions given as a semver pattern, and never to both.
A range artefact covers every version its pattern matches, so a file that does not change between releases is published once instead of per release.

An artefact carries a type and a platform, which together say what it is and what it is for.
A version's artefacts are the exact-version ones plus every range artefact whose pattern matches it.

## What a version offers

Canopy offers one artefact per type and platform, choosing the most specific of those that match.
An exact-version artefact is more specific than any range artefact; between two ranges, the narrower is more specific.
A pattern Canopy cannot parse matches nothing rather than everything, so a malformed range withholds a file instead of offering it to the whole fleet.

The full set, including the artefacts specificity passed over, is available to operators.
What resolution hides is a fact about how a version was published and an operator has to be able to see it.

## Group-scoped artefacts

An artefact may belong to a group, and one that does is offered only to that group.
An artefact belonging to no group is offered to every group, which is what a version's installers, migrations, and manifests are: properties of the version and of nothing narrower.

Belonging to a group is therefore an addition rather than a requirement, and the artefacts published today are unaffected.
Resolution keys on type, platform, and group together, so a group-scoped artefact and an unscoped one of the same type do not displace each other and each group is offered its own.

A group-scoped artefact is offered only where the caller's group is known.
A caller whose credential carries a group, a server device being the case that matters, never names one and is answered for its own group alone, so a server cannot ask what another group is offered.
A caller carrying no group of its own names the group it asks about and is answered only for a group it is authorised for, which is how a build reads what it is about to replace and how an operator reads the fleet.
A read carrying no identity at all is answered with the unscoped artefacts alone, so giving an artefact a group narrows who is offered it rather than widening what an open path serves.
A group-scoped artefact's existence is disclosed only to a caller it is offered to: a caller that names or guesses one it is not offered is answered as though it did not exist, so which groups hold one is not enumerable through the artefact surface.

A group scope exists because some artefacts are derived from a group's own data and are wrong for anyone else, a reporting schema being the case that motivates it (see [RPT](../public-server/reporting-schemas.md)).
Such an artefact is published into that group's own object storage, over a credential Canopy issues the publisher for the run, and is read back through Canopy, which passes it only to a caller the artefact is offered to.
The boundary is therefore enforced on the read rather than resting on a location being hard to guess.

## Registration

A releaser device registers artefacts against a version over its own credential, which is what a product's release automation uses when it publishes.
Registering a group-scoped artefact requires being authorised for that group, and the group is named on the registration rather than inferred from the caller.
A restore consumer registers the artefacts its builds produce for the groups its own declarations authorise it to read, so a build needs no releaser credential to publish what it made.
A releaser credential carries no group and registers unscoped artefacts alone, which is what a version's installers, migrations, and manifests are.
An operator may also record one directly.
Canopy records which device registered an artefact and, where the registration names one, the run that produced it, so an artefact that arrived by automation is distinguishable from one entered by hand and traceable to what made it.

## What Canopy does not know

Canopy holds where an artefact is published and not what it contains.
It does not go looking for the file to check it, so an artefact whose location stops resolving is not detected until something tries to read it, and two artefacts published to the same location with no digest between them are the same artefact to Canopy however their contents differ.

An artefact carries a digest where whoever registers it records one, and a group-scoped artefact carries one always, because what a server has applied is graded against the artefact Canopy holds and successive builds for one version are otherwise indistinguishable (see [RPT](../public-server/reporting-schemas.md)).
Where Canopy passes an artefact through and holds a digest for it, it verifies the bytes against that digest and refuses them on a mismatch, so a file replaced at its published location fails the read rather than reaching a server as the artefact it is not.

A version's publication is corroborated against the artefacts Canopy holds for it rather than against the files themselves, which is enough to say a version has published what it needs to and not enough to say those files are good.

## Out of scope

- Hosting or retaining the files themselves: Canopy passes bytes through on request and keeps no copy of them.
- What any artefact contains, and whether it is fit for what fetches it.
- Which artefacts a product must publish to be considered released.
