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

Removing a zone from Canopy's configuration does not remove the claims within it.
Such a claim is kept and reported as unmatched: Canopy will act on no name beneath it, though it still holds its exclusivity against other claims, since the claim itself has not been given up.
An operator resolves an unmatched claim by restoring the zone to the configuration or by releasing the claim.

Archiving a group keeps its domains, so restoring the group restores its control of them.

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

A group presents the domains it controls, each with the managed zone it resolves to, and flags a claim no configured zone matches.
The configured managed zones are shown to operators, so an operator claiming a domain can see which names are available to be claimed.

A server presents whether it may manage its own DNS and whether it may obtain its own certificates, alongside the other permissions an operator holds over it, and an operator grants and revokes each there.
