# Audit Log Unavailability System Log Record

This document owns the safe content and normal-operation handling of the
**[System Log](../../glossary.md#applications-and-interfaces)** record produced
when an **[Audit Log](../../glossary.md#applications-and-interfaces)**
destination cannot accept a required record. The **[Log Module](../../glossary.md#applications-and-interfaces)** [Design](../../log-modules/log-module-design.md)
owns the shared record envelope, destination contract, and classification
taxonomy. The [Audit Log Design](../audit/audit-log-design.md) owns Audit record
construction, synchronous delivery, and terminal recovery behavior; this
document does not authorize or sequence an application mutation.

## Construction And Safe Context

`weavelit-server-observability` constructs a `System` record with result
`Failure` and classification `dependency.audit-log-unavailable`. Its variable
context is limited to two already validated values:

- the lowercase kebab-case Log Module identifier for the assigned destination;
  and
- the closed typed Audit classification for the affected operation, or the
  fixed `internal.log-policy.changed` activity context for terminal recovery.

The detail has the fixed form `audit destination module <module> unavailable
for <audit-classification>`. The Server-issued record identifier, event time,
and response correlation identifier fill the shared envelope.

No `LogDeliveryError`, destination error text, Audit record body, request
payload, target, principal, credential, database or filesystem detail, or
arbitrary operation text enters construction. Record-construction errors remain
payload-free and cannot render any supplied event value.

## Delivery And Consequential Rejection

After synchronous Audit delivery fails, the owning workflow may call the
normal-operation support with the failure and the typed context above. The
support consumes but never inspects or renders the delivery error. It constructs
the System record once and makes at most one synchronous delivery attempt to the
configured System Log destination. It does not retry, queue, schedule, persist
an outbox entry, or create substitute Audit evidence.

System record construction or delivery failure is absorbed at this reporting
boundary. The original consequential action still receives the payload-free
`audit_log_unavailable` error with the stable message `Audit Log unavailable;
operation rejected.` The helper returns normally and remains usable after a
failure, so an unavailable Audit or System destination does not crash or exit
the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**.
Transport-specific status and response mapping remain with the future owning
route.

This support applies only when an owning workflow has already decided the action
is consequential. It neither authorizes nor mutates state. It does not decide
whether a nonconsequential operation may absorb an Audit failure, and it does
not imply that such an operation was audited; each future route must make that
choice under the Technical Specification.

The operational terminal-recovery coordinator uses the same construction and
one-attempt delivery core for assignment resolution, obligation listing or
import, Audit delivery, and database acknowledgement failures. It generates a
fresh record identifier and event time, uses the fixed correlation identifier
`audit-terminal-recovery`, and returns no client mapping. It invokes reporting
once for each encountered failure, absorbs construction or System destination
failure, and never re-enters Audit recovery. Raw dependency errors, committed
settings, and obligation projections do not enter this path.
Successful assignment resolution and empty active or late-delivery sequence
reads are healthy results. Activation reports both empty sequences as `ready`
and does not construct a `dependency.audit-log-unavailable` System record.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Audit Log Design](../audit/audit-log-design.md)
- [Authorization-Denial System Log Record](authorization-denial-record-design.md)
- [Glossary](../../glossary.md)
- [Testing and Validation Policy](../../testing.md)
