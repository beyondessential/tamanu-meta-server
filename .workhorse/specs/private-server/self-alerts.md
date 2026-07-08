---
id: SELF
---

# Self-alerts

A self-alert is Canopy reporting a problem with its own operation, as distinct from an issue observed on a fleet member.
Self-alerts are Canopy-wide checks (see [CHK](../monitoring/checks.md)): they carry the same state, severity catalog, silences, and resolution as any other check, and they aggregate into incidents on the Canopy target (see [INC](../monitoring/incidents.md)).

## Conditions

Each self-alert condition is a check with a stable name under the `canopy` source, and at most one state exists per condition: repeated detections update the one state rather than accumulating.
A condition is active while it holds, and recovers when it clears; a condition without automatic recovery stays active until an operator resolves it.

The current conditions are:

- Canopy's own identity for reaching backup storage is broken (critical).
- An operator-notification delivery has permanently failed (stays until operator-resolved).
- An MCP access token is within fifteen days of its expiry (see [MCP](mcp.md), "Access tokens").

## Notification

Self-alerts notify through the incident machinery: an error-or-worse condition opens an incident on the Canopy target, which notifies the operator channel per the incident notification rules (grace period, critical bypass, escalation, recovery notice).
When the notification channel is not configured, self-alerts are still recorded and presented, and the operator is warned that notification is off.

## Presentation

Active self-alerts are presented on their own surface in the operator UI, apart from fleet issues and incidents: a persistent notice visible from any page while any alert is active, leading to a view of the active alerts and recent recoveries.
Self-alerts do not appear in the fleet issue listings, and are not presented as belonging to any server.
Each alert presents its severity, when it became active, and a description of the condition and what to do about it.
