---
id: CRT
---

# Server names and certificates

A server reaches Canopy for the two things it cannot do for itself about its own public name: publishing the address records that make the name resolve, and obtaining a TLS certificate for it.
Both are confined to names within the domains the server's group controls, and both are refused unless an operator has granted that server the matching permission (see [DOM](../servers/domains.md)).

Canopy is the only holder of DNS write access and of the certificate authority account.
A server holds neither, which is the point: a fleet where every server carried zone credentials would put the whole zone at the mercy of its least-defended member.

## Why Canopy issues

A server's name resolves to an address that may not be reachable from the public internet, and often is not: a facility server sits behind someone else's NAT.
So the challenge types that prove control by answering on the name's own address are unavailable, and proving control through DNS is the only route left.
Proving control through DNS means writing to the zone, and Canopy already holds that access on the group's behalf.

Centralising issuance also puts the authority's rate limits, the record of every certificate's expiry, and the alerting when renewal stops working in one place — the same place that already watches the fleet.

## Identity and authorisation

A certificate or address request authenticates as the device enrolled against the server it concerns, by either transport Canopy already accepts for devices (see [DID](device-identity.md)).

Every request is checked in the same order, and each check is reported distinctly so a misconfiguration is diagnosable from the refusal alone:

1. The caller authenticates as a device attached to a live server.
2. That server has the grant the request needs — DNS management for addresses, certificate issuance for certificates.
3. The requested name lies at or beneath a domain the server's *own group* controls.
4. A managed zone covers that domain, so Canopy can act on the name at all.

A name within another group's domain is refused as if unclaimed: the refusal says the server's group does not control the name, and never that another group does, so the endpoint is not a directory of other deployments' names.

A certificate for a name is available to any server of the group that controls it, provided that server holds the certificate grant.
Certificate issuance is deliberately not tied to which server registered the name's addresses: a standby that serves the same name on failover needs the same certificate, and Canopy holds no view of which member is live.

## What a server may act on

A server can ask Canopy what names it is entitled to, rather than discovering the boundary by being refused.
The answer carries the domains its group controls, which of the two grants it holds, and the names it already has addresses registered or certificates issued for, each with when the certificate expires.

That is enough for an agent to work ahead of demand: knowing the domains it may use and what it already holds, it can request a certificate before anything asks for one, and renew before expiry, instead of discovering at handshake time that it has nothing to serve.

The answer is available both on its own and on the response to a status push, so an agent that already reports status learns of a new domain or a newly granted permission without asking separately (see [STA](statuses.md)).
Both carry the same content, the standalone form being for an agent that wants it without pushing.

A server with no grants, or whose group controls no domain, is told so plainly: the answer is empty rather than an error, since asking what one may do is not itself a privileged act.

## Addresses

A server registers the name it should be reachable at together with the external addresses it is reachable at, and Canopy publishes the address records: the IPv4 addresses as A records, the IPv6 addresses as AAAA records, at that name, in the managed zone the name resolves to.

Registering replaces the addresses previously registered for the name, so a server announces a change of address by registering again, and a registration naming no addresses withdraws the name.
Canopy publishes what it is told: it does not verify that an address is really the server's, the grant being the trust boundary rather than any proof of possession.

Canopy changes only records it created itself.
Because zones are shared, a name may be served by records Canopy knows nothing about, and Canopy neither rewrites nor removes those; it records what it has published so it can tell its own records from everyone else's.

A name's addresses are registered by one server at a time.
While a registration stands, another server registering the same name is refused, so two members of a group cannot fight over where one name points.

## Certificates

### Requesting

A server generates its own key pair and asks Canopy to certify it, submitting a certificate signing request for a single name.
The private key never leaves the server and Canopy never asks for it: Canopy's part is to prove control of the name and return the signed chain.

The signing request is honoured only for exactly the name requested.
Canopy certifies that one name and no other: a request whose subject or alternative names carry anything beyond the requested name is refused rather than trimmed, because silently issuing something narrower than asked would leave a server serving a certificate it does not expect, and issuing something wider would let one server smuggle a second deployment's name past the authorisation check.
Wildcards are refused: a certificate valid for every name in a deployment is not something one member should be able to mint.

The key the request certifies must be strong enough to be worth certifying, and Canopy states what it accepts rather than deferring to whatever the authority happens to allow that year.

### Fulfilment is not immediate

Proving control through DNS takes as long as it takes for the authority to see a record Canopy has just published — tens of seconds at best, minutes when a resolver holds a negative answer.
That is far longer than any client will wait mid-handshake, so requesting a certificate and collecting it are separate steps: a request is accepted and acknowledged, Canopy works the order in the background, and the server collects the result when it is ready.

A server therefore holds a certificate before it needs one, rather than obtaining one at the moment a client arrives.
Canopy's contract is only that a request is durable once accepted and that its outcome becomes collectable; scheduling requests early enough to be useful is the server's business.

Repeating a request for a name Canopy already holds a valid certificate for returns the one it holds rather than ordering another, so a server that has lost its local copy — restarted, redeployed, cache cleared — is served without spending the authority's budget.
A request naming a key different from the one already certified is a new order, since the stored chain certifies a key the server no longer holds.

### What Canopy keeps

Canopy keeps the certificate it obtained, the name it covers, the server it was issued for, and when it expires.
It keeps no private key, having never held one.

Holding the chain is what lets Canopy answer a repeat request without a fresh order, renew before expiry without being asked, and report a certificate that is running out.

### Renewal

Canopy renews a certificate it holds before it expires, without being asked, and the renewed chain becomes collectable the same way the first one did.
A server that collects periodically therefore stays current without tracking expiry itself, and one that asks again gets whatever is newest.

Renewal stops when the certificate is no longer wanted: a name whose group has released the domain it sits under is not renewed, nor is a certificate for a server whose grant has been revoked or that has been archived.
A grant revoked does not withdraw the certificate already issued — it cannot be recalled once it exists — but it does end the renewals that would extend it.

### When issuance fails

An order that fails is retried, with the interval between attempts growing, because most failures are the authority being briefly unavailable or a record not yet visible.

A certificate approaching expiry that has not been renewed is Canopy failing at something it took responsibility for, so it is reported as a self-alert against Canopy rather than as an issue on the server (see [SELF](../private-server/self-alerts.md)).
It names the certificates concerned and how long each has left, escalating as the remaining life shortens, and clears when they are renewed.
A first-time order that keeps failing is reported the same way, distinguished from a renewal so an operator can tell a deployment that never came up from one about to go dark.

The authority's own rate limits are shared across every group whose domain sits in the same zone, so exhausting them is a fleet-wide fault rather than one group's: Canopy reports being throttled, and does not consume the remaining budget retrying a name that has just failed.

## Presentation

A server presents the certificates Canopy holds for it, each with the name it covers and when it expires, and a request that has not yet produced one presents as pending or as failed with the reason.
An operator can see, per group, which names have certificates and which are overdue, without going to the servers themselves.
