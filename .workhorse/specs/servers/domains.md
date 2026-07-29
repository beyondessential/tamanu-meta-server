---
id: DOM
---

# Server group domains

Canopy manages DNS names on its fleet's behalf.
A *managed zone* is a DNS zone Canopy can create and change records in; a *group domain* is a name inside such a zone that a server group controls.
Together they answer which names Canopy will act on for which deployment — the association a server's own DNS records and TLS certificates are authorised against.

## Managed zones

A managed zone is named by its apex domain and identified by the identifier its DNS provider knows the zone by.
Canopy holds write access to every managed zone, and can create, change, and delete records anywhere within it.

Managed zones come from Canopy's own deployment configuration, set by the infrastructure that provisions Canopy, rather than from operator-editable state.
An operator cannot add, change, or remove a managed zone through Canopy: a zone Canopy has not actually been granted write access to would be authority Canopy could not honour, and granting that access is the provisioning infrastructure's job.
Canopy reads the configured zones when it starts, so a change to the configuration takes effect once Canopy restarts.

A managed zone is not dedicated to a single deployment.
Zones are shared: the domains of several groups live in one zone, and names Canopy does not manage at all may live in it beside them.
So Canopy never treats a zone as belonging to a group, and never assumes it is the only writer of records in a zone.

## Group domains

A group domain is a domain name that a server group controls.
A group may control more than one domain, and controls everything beneath each of them.

A group domain lies within a managed zone: it is either a zone's apex or a name beneath one.
A name outside every managed zone cannot be claimed, because Canopy could create no records for it.
Claiming an apex gives the group the whole zone, which is how infrastructure that means to dedicate a zone to one deployment configures it.

A group domain resolves to exactly one managed zone: the longest configured apex the domain lies within, so a zone configured beneath another zone takes the names beneath itself.

## Exclusivity within Canopy

Within Canopy, a group domain is exclusive.
No two group domains overlap: claiming a name already claimed, a name beneath an existing claim, or a name above an existing claim is refused, whether the existing claim belongs to the same group or to another one.
The refusal names the overlapping domain and the group holding it, so an operator can see whom to ask.

Exclusivity is Canopy's own rule and not an assertion about the wider DNS.
A domain a group controls in Canopy may also be served by systems outside Canopy, and Canopy neither detects nor prevents that.
What Canopy guarantees is that no other group within Canopy holds a name overlapping it.

A name is controlled by a group when it is at or beneath one of that group's domains.
Because claims never overlap, at most one group controls any given name, and that group is the one every feature acting on the name is authorised against.

## Claiming and releasing

An operator claims a domain for a group and releases it again; both are administrative actions, while reading a group's domains and the configured zones is open to any operator.
A claim records the operator who made it and when.

A claimed name is normalised: case and a trailing dot are not significant, and the name is held in lower case without one.
A name is claimed in the form DNS uses, so an internationalised domain is claimed in its ASCII-compatible form rather than its unicode spelling.
A claim is refused when the name is not a syntactically valid domain name of at least two labels.

Releasing a domain ends the group's control of it and of every name beneath it.
Releasing is the only way a claim goes away: nothing Canopy observes about its own configuration ever drops one.

Archiving a group keeps its domains, so restoring the group restores its control of them.

## When the zone configuration changes

A claim outlives the zone that admitted it.
Removing a zone from Canopy's configuration — or breaking the configuration outright — leaves every claim inside it standing, still excluding other groups from overlapping it, and still the group's to release.
What stops is Canopy acting: no name beneath an uncovered claim resolves to a zone, so Canopy publishes and renews nothing there.
Canopy holds the claim rather than dropping it because a claim is an operator's decision about a deployment, and a configuration Canopy cannot read is no evidence that the decision was withdrawn — inferring release from it would silently hand a live deployment's names to whoever asked next.

Because the configuration is the only thing that changed, and it is a Canopy-level fault rather than any one group's, Canopy reports the shortfall as a self-alert against itself rather than as an issue on the affected groups (see [SELF](../private-server/self-alerts.md)).
One alert covers every uncovered claim at once and names each domain with the group holding it, so an operator sees the whole blast radius in one place instead of visiting each group.
The alert distinguishes what happened, because the two cases are fixed differently:

- Some claims are uncovered while other zones remain configured, which is what removing one zone of several looks like.
  This is a warning: the rest of the fleet is unaffected, and an operator either restores the zone or releases the claims that no longer belong.
- The configuration cannot be read, or names no zones at all while domains stand claimed.
  This is a failure: Canopy can act on no group's names, so it reports the parse error where there is one, and says it is treating itself as having no zones.

A configuration that names no zones while nothing is claimed raises nothing, that being the feature simply not in use rather than a fault.
An archived group's uncovered claims raise nothing either, a deployment that has been put away not being something to call an operator about.

The alert recovers on its own once every live group's claims sit within a configured zone again, whether that came about by restoring the zone or by releasing the claims.
A group's own page flags each of its uncovered claims besides, so an operator arriving from the alert sees which of that group's domains are the problem.

## Permission for a server to manage its own names

A server manages names under its group's domains only where an operator has explicitly permitted it to.
The permission is two separate grants, held per server and both withheld by default: one to manage the server's own DNS records, and one to obtain TLS certificates for the server's own names.
They are separate because a deployment whose records are managed elsewhere may still want its certificates from Canopy, and a deployment may be given a name before it is trusted to hold a certificate for it.

Both grants are per server rather than per group, so one member of a group may manage its names while its neighbours may not.

A server that has not been granted the permission it needs is refused, and told that it is: the request is authenticated as the server it claims to be, and denied on the permission rather than ignored, so an operator reading the server's logs sees a permission to grant rather than a silence to explain.
A server whose group controls no domain is likewise refused, but for want of a domain rather than for want of permission, so the two misconfigurations are told apart.

Revoking a grant takes effect on the server's next request.
It stops the server making further changes and leaves the records and certificates already in place, since withdrawing a live deployment's address records on a change of permission would take that deployment off the air.

## Server-registered names and addresses

A server permitted to manage its DNS registers a name it should be reachable at, along with the external addresses it is reachable at, and Canopy publishes that name's address records.

The name must lie within one of the server's own group's domains.
A name within another group's domain, or within no group's domain, is refused, so the group domain is the boundary of what a server can reach.

Canopy publishes the IPv4 addresses given as A records and the IPv6 addresses as AAAA records, at the name registered, in the managed zone that name resolves to.
Registering replaces the addresses previously registered for that name, so a server announces a change of address by registering again, and a registration that names no addresses at all is a request to withdraw the name.

Canopy changes only the records it created itself.
Because zones are shared, a name in a managed zone may be served by records Canopy knows nothing about, and Canopy neither rewrites nor removes those.

A name is registered by one server at a time: while a registration stands, another server registering the same name is refused, so two members of a group cannot fight over one name.

Canopy does not check that a registered address is really the server's.
The permission is the trust boundary: a server permitted to manage its DNS can point a name under its group's domains at any address, and a server not permitted to can point nothing anywhere.

## Presentation

A group presents the domains it controls, each with the managed zone it resolves to, and flags a claim no configured zone covers.
The configured managed zones are shown to operators, so an operator claiming a domain can see which names are available to be claimed.

A server presents whether it may manage its own DNS and whether it may obtain its own certificates, alongside the other permissions an operator holds over it, and an operator grants and revokes each there.
