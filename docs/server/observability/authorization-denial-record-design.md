# Authorization-Denial System Log Record

This document owns the fixed content of the System Log record produced for one
denied authorization decision. It is one instance of the general System Log
record envelope, classification taxonomy, and pre-redaction design owned by the
[Log Module Design](../../log-modules/log-module-design.md); it does not restate
that envelope's shared fields or bounds. The
[Authorization Design](../authorization/authorization-design.md) owns when and
why a request is denied; this document owns only what gets recorded about that
denial.

## Construction

`weavelit-server-observability`'s `authorization` module builds the record.
Given a fresh record identifier, an event time, and the correlation identifier
the denial response already reports, it produces a `System` record whose result
is always `Failure` and whose classification and detail are the fixed constants
below. No other field varies.

| Field | Value |
| --- | --- |
| `classification` | `authorization.denial` |
| `detail` | `request authorization denied` |

Both values are compile-time constants with no interpolation. The Server-issued
record identifier, the event time, and the correlation identifier are therefore
the only fields that differ between two denials.

Nothing else may appear in the record. No account, username, session,
**[Client Module](../../glossary.md#applications-and-interfaces)**,
**[User Plane](../../glossary.md#applications-and-interfaces)** or
**[Administration Plane](../../glossary.md#applications-and-interfaces)**,
**[Service Module](../../glossary.md#applications-and-interfaces)**,
**[Operation](../../glossary.md#applications-and-interfaces)**,
**[Group](../../glossary.md#identities-and-access)**, grant, enablement state,
**[Service Connection](../../glossary.md#applications-and-interfaces)**, request value,
or internal reason reaches it. An inactive **[Human User](../../glossary.md#identities-and-access)**,
a disabled or uncatalogued component, and every missing grant are denied for
different reasons, but all produce this one identical record, so the
**[System Log](../../glossary.md#applications-and-interfaces)** cannot separate
them either.

The `classification` value is the registered System Log taxonomy entry in the
[Log Module Design](../../log-modules/log-module-design.md#event-classification-taxonomy).
A destination stores a classification opaquely and tolerates an additively
registered value, so an unregistered classification would still be accepted and
stored. Nothing at the destination would reject it, which is why this value is
pinned to the taxonomy in a test against the literal string rather than against
the constant the producing crate declares.

## Delivery Timing And Failure Absorption

The Server-owned route layer, not this crate, decides when to attempt delivery
and how to handle a failure to do so. It attempts delivery to the configured
System Log destination before the denial is returned to the caller, so the
record reflects an attempt already made rather than a background task racing the
response.

An unconfigured System Log destination, a clock or randomness failure while
preparing the record, and a delivery failure are all absorbed silently. Each of
these leaves the denial exactly as it would have been without recording: no
failure inside recording can turn a denial into an allow, change which reason is
reported, enrich the response, delay it beyond the request's existing processing
budget, or otherwise change what the caller receives.

## Related Documents

- [Authorization Design](../authorization/authorization-design.md)
- [Authentication-Failure System Log Record](authentication-failure-record-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
