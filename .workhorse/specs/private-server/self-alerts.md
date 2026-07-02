---
id: SELF
---

# Self-alerts

A self-alert is Canopy reporting a problem with its own operation, as distinct from an issue observed on a fleet member.

## Conditions

Each self-alert condition is identified by a stable reference, and at most one alert exists per condition: repeated detections coalesce into the one alert rather than accumulating.
An alert is active while its condition holds, and recovers when the condition clears; a condition without automatic recovery stays active until an operator resolves it.

The current conditions are:

- Canopy's own identity for reaching backup storage is broken (critical).
- An operator-notification delivery has permanently failed (stays until operator-resolved).
- An MCP access token is within fifteen days of its expiry (see [MCP](mcp.md), "Access tokens").

## Notification

An active self-alert notifies operators over the operator-notification channel directly: it does not open an incident and does not attach to any fleet group.
One notification is sent when an alert becomes active, and one when it recovers.
Alerts below critical severity wait out a short grace period before notifying, and an alert that recovers within that grace sends nothing at all; critical alerts notify immediately.
While an alert stays active, repeated detections send no further notifications.
When the notification channel is not configured for self-alerts, they are still recorded and presented, and the operator is warned that notification is off.

## Presentation

Active self-alerts are presented on their own surface in the operator UI, apart from fleet issues and incidents: a persistent notice visible from any page while any alert is active, leading to a view of the active alerts and recent recoveries.
Self-alerts do not appear in the fleet issue listings, and are not presented as belonging to any server.
Each alert presents its severity, when it became active, and a description of the condition and what to do about it.
