---
id: STA
---

# Status reporting

A server device sends Canopy periodic status heartbeats: a self-reported health flag, named health checks, and free-form host facts (versions, uptime, and the like).
Canopy records each, derives incidents from failing checks, and infers reachability from how recently a device reported.

More than one agent may report for one host: on a Tamanu server the bestool alert daemon and Seedling run against the same device identity, each with its own view.
This spec covers how Canopy keeps those reports distinct and replies to each.

## Scope

This spec covers the device-facing status contract: how a heartbeat is attributed to a reporting agent, how Canopy keeps concurrent agents under one server distinct, and what it returns in response.

It does not cover how a device enrols or authenticates (see [DTR](../private-server/device-trust.md)), nor what a device backs up (see [BAK](backup.md)).

## Identity and clients

A heartbeat authenticates as a server device over either transport Canopy accepts, exactly as every device request does; identity is never taken from the request body.

Under one authenticated server, a heartbeat is attributed to a named **client**, the agent that produced it.
The set of client names is open; the two Canopy expects are `bestool` (the alert daemon) and `seedling`.

A heartbeat that names no client is attributed to `bestool`, so agents already deployed, which name none, keep reporting unchanged until they are rebuilt.

The client is a label, not a second identity: authorisation stays the server binding, so a device reports only for the server it is bound to, and an admin for any.

## Independent streams

Canopy records status per `(server, client)`, so two agents reporting for one server never overwrite each other: each keeps its own latest heartbeat, health checks, and history.

A server is not treated as down while any of its clients is still reporting, so one agent going quiet does not by itself make the server look down.
A quiet agent is still surfaced: when the server is reporting but its `bestool` client — the stream Canopy's health view reads — has gone quiet past the server's down threshold, Canopy raises an issue for that client, distinct from server reachability, and resolves it when the client reports again.
A server that has never had a `bestool` client raises nothing.

## Response

Canopy knows the reporting client from the request, so it returns only what that client needs, and does not give one client another's concerns.
A client acts on the parts of the response it understands and ignores the rest; it is sent nothing meant only for another client, and relies on receiving nothing beyond its own.

The backup types due now go to the client that runs backups (`bestool`), not to one such as Seedling that does not.
A client that reports health checks is told the severities Canopy applies to them.
The response carries only such return-path instructions; the recorded status itself is not echoed back.
