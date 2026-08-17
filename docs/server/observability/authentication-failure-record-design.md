# Authentication-Failure System Log Record

This document owns the fixed content of the System Log record produced for one
denied local authentication attempt. It is one instance of the general System
Log record envelope, classification taxonomy, and pre-redaction design owned by
the [Log Module Design](../../log-modules/log-module-design.md); it does not
restate that envelope's shared fields or bounds. The
[Server Authentication Design](../authentication/authentication-design.md) owns
when and why an attempt is denied; this document owns only what gets recorded
about that denial.

## Construction

`weavelit-server-observability`'s `authentication` module builds the record.
Given a fresh record identifier, an event time, and the correlation identifier
the denial response already reports, it produces a `System` record whose result
is always `Failure` and whose classification and detail are the fixed constants
below. No other field varies.

| Field | Value |
| --- | --- |
| `classification` | `authentication.failure` |
| `detail` | `local password authentication denied` |

Both values are compile-time constants with no interpolation, so no username,
account identifier, password, token, address, or other client-supplied text can
reach the record through them. An unknown account, an inactive account, an
account with no usable verifier, and a wrong password are denied by the
**[Local Authentication](../../glossary.md#identities-and-access)** decision for
different reasons, but all four produce this one identical record.

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
record reflects an attempt already made rather than a background task racing
the response.

An unconfigured System Log destination, a clock or randomness failure while
preparing the record, and a delivery failure are all absorbed silently. Each of
these leaves the denial exactly as it would have been without recording: no
failure inside recording can enrich, delay beyond the request's existing
processing budget, or otherwise change what the caller receives.

## Related Documents

- [Log Module Design](../../log-modules/log-module-design.md)
- [Server Authentication Design](../authentication/authentication-design.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
