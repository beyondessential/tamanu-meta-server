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

A server reports whether it runs Munin as a field in its pushed status.
Canopy remembers the value with grace: the most recently reported value persists, a status that omits the flag leaves it unchanged, and the value is not bound by the window that governs a server's live status.
So once a server has reported running Munin the link stays available even after the server stops reporting, and a server that reports it has stopped running Munin loses the link.

The detail view offers a link to a server's Munin only when the server is known to run Munin and has a bound tailnet name.
The link opens the server's Munin in a new tab, over HTTPS at the server's tailnet MagicDNS name on port 4950.
A server not known to run Munin, or without a tailnet name, offers no Munin link.
