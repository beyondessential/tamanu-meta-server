---
id: SVC
---

# Server service links

A server's detail view links an operator out to the web services running on that server, so an operator can jump from Canopy straight to the server's own interfaces.
How a server reports its metadata is the status contract (see [STA](../public-server/statuses.md)); the tailnet identity these links resolve against is the device-trust model (see [DTR](device-trust.md)).

## Application link

When a server has a known address — its configured URL, or its bound device's tailnet name when it has no configured URL — the detail view offers a link that opens the server's application in a new tab.
A server with no known address offers no application link.

## Munin link

A server reports whether it runs Munin as a field in its pushed status, and the flag is one of the server's reported figures (see [FIG](server-figures.md)) — read from the most recent source to report it, and held for as long as the server exists.
So a server that has reported running Munin keeps the link however long it has been quiet since, a status that omits the flag leaves the value unchanged, and a server that reports it has stopped running Munin loses the link.

The detail view offers a link to a server's Munin only when the server is known to run Munin and has a bound tailnet name.
The link opens the server's Munin in a new tab, over HTTPS at the server's tailnet MagicDNS name on port 4950.
A server not known to run Munin, or without a tailnet name, offers no Munin link.
