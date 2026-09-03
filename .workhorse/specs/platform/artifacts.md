---
id: ART
---

# Version artifacts

An artifact is a file published for a version: an installer, a package, a set of migrations, a manifest.
Canopy is the index of them: it holds where each one is, what it is, and whom it is for, and a server or the infrastructure acting on a version's behalf learns from Canopy which files a version has and fetches them from where they rest.
An artifact may be for one group alone, and Canopy offers it only to a caller whose group it is.

## What an artifact belongs to

An artifact belongs to one exact version, or to a range of versions given as a semver pattern, and never to both.
A range artifact covers every version its pattern matches, so a file that does not change between releases is published once instead of per release.

An artifact carries a type and a platform, which together say what it is and what it is for.
A version's artifacts are the exact-version ones plus every range artifact whose pattern matches it.

An artifact may belong to a group, and one that does is for that group alone.
An artifact belonging to no group is for every group.
A group scope exists because some artifacts are derived from a group's own data and are wrong for anyone else.

## Where an artifact rests

An artifact belonging to no group rests at a URL, which whoever is offered the artifact fetches directly.

A group-scoped artifact rests as an object in its own group's storage, under a prefix of its own apart from the group's backup repo, and a registration placing one anywhere else is refused.
Canopy reads it on a caller's behalf by assuming the group's storage role confined to reading that prefix (see [BAK](../public-server/backup.md)), and streams the bytes to the caller, so the file rests only in the group's storage and is readable only through Canopy.
The boundary is therefore enforced on the read rather than resting on a location being hard to guess.

Canopy issues a publisher short-lived credentials for writing into a group's artifact prefix the way it issues backup credentials: by assuming the group's storage role under a session policy confined to that prefix, recorded before they are returned, and only to a caller authorised to register artifacts for that group (see [Registration](#registration)).
The prefix is apart from the backup repo so that a credential which writes artifacts reaches no backup.

## What a version offers

Canopy offers a caller one artifact per type and platform, chosen from the artifacts that caller may see: those belonging to no group, and those scoped to the caller's group where that group is known.
Where several match, the most specific is offered.
An artifact scoped to the caller's group is more specific than one belonging to no group.
Among artifacts of the same scope, an exact-version artifact is more specific than any range artifact, and between two ranges the narrower is more specific.
A group-scoped artifact and an unscoped one of the same type and platform are therefore both held, each group is offered the one for it, and no caller is offered both.
A pattern Canopy cannot parse matches nothing rather than everything, so a malformed range withholds a file instead of offering it to the whole fleet.

The full set, including the artifacts specificity passed over, is available to operators.
What resolution hides is a fact about how a version was published and an operator has to be able to see it.

## Who is offered a group-scoped artifact

A caller whose credential is bound to a server has that server's group, where the server has one.
It never names a group and is answered for its own alone, so a server cannot ask what another group is offered.
A caller carrying no group of its own names the group it asks about and is answered for a group it is authorised for: an operator for any group, and a component that produces or applies a group's artifacts for that group, as defined with those artifacts.
A read carrying no identity, or naming no group, is answered with the unscoped artifacts alone, so giving an artifact a group narrows who is offered it rather than widening what an open path serves.
A group-scoped artifact's existence is disclosed only to a caller it is offered to: a caller that names or guesses one it is not offered is answered as though it did not exist, so which groups hold one is not enumerable through the artifact surface.
Canopy passes a group-scoped artifact's bytes only to a caller it is offered to.

## Registration

A registration names the version or range, the type, the platform, the location, and the group where the artifact has one.
The group is named on the registration rather than inferred from the caller.

A releaser device registers unscoped artifacts, and carries no authorisation for any group.
An operator registers either.
A component that produces a group-scoped artifact registers it for that group under an authorisation defined with that artifact, and is authorised for no other.
A registration naming a group the caller is not authorised for is refused.
Credentials for writing into a group's storage are issued on the same authorisation, so a caller that may not register for a group cannot publish into it either.

Canopy records which device registered an artifact and, where the registration names one, the run that produced it, so an artifact that arrived by automation is distinguishable from one entered by hand and traceable to what made it.

## Digests

An artifact carries a digest where whoever registers it records one, and a group-scoped artifact carries one always, since its bytes reach a caller through Canopy and are verified on the way.
Where Canopy passes an artifact through and holds a digest for it, it verifies the bytes against that digest and refuses them on a mismatch, so a file replaced at its published location fails the read rather than reaching a server as the artifact it is not.
