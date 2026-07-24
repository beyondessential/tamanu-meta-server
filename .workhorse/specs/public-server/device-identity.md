---
id: DID
---

# Device self-identity

A device authenticates to the device API by presenting its enrolled certificate, and Canopy resolves its identity from that certificate rather than from any identifier the device sends.
A device therefore never needs to know its own device or server identifiers to make authenticated calls, but it is assigned both when it completes enrollment (see [STA](statuses.md) for the enrolled-device contract).
A device that has lost track of those identifiers can recover them from the device API.

## Query

A device asks the device API for its own identity by calling `GET /servers/self`.
The caller presents its enrolled device certificate, or a certificate holding the admin role.

The response carries the device's own identifier and the identifier of the server it is enrolled as — the same pair returned when the device completed enrollment.

The request is refused when the caller presents no recognised certificate, when the resolved device is not attached to any server, and when it is attached to more than one server.
