---
id: DID
---

# Machine self-identity

A machine authenticates to the device API by presenting its enrolled certificate, and Canopy resolves its identity from that certificate rather than from any identifier the machine sends.
A machine therefore never needs to know its own identifiers to make authenticated calls, but it is assigned them when it completes enrollment (see [STA](statuses.md) for the enrolled contract).
A machine that has lost track of them can recover them from the device API.

## Query

A machine asks the device API for its own identity by calling `GET /machines/self`.
The caller presents its enrolled certificate, or a certificate holding the admin role.

The response carries the identity's own identifier, the machine it is enrolled as, and the applications Canopy currently holds for that machine.

An identity belongs to at most one machine (see [FLT](../servers/overview.md), "Cardinality"), so the answer is never ambiguous.
The request is refused only when the caller presents no recognised certificate, or when the resolved identity belongs to no machine.

`GET /servers/self` reaches the same answer, kept for callers that predate the machine model and marked deprecated.
