# Audit Log Unavailability System Log Record

This document owns the safe content and normal-operation handling of the
**[System Log](../../glossary.md#applications-and-interfaces)** record produced
when an **[Audit Log](../../glossary.md#applications-and-interfaces)**
destination cannot accept a required record. The **[Log Module](../../glossary.md#applications-and-interfaces)** [Design](../../log-modules/log-module-design.md)
owns the shared record envelope, destination contract, and classification
taxonomy. The [Audit Log Design](../audit/audit-log-design.md) owns Audit record
construction and synchronous delivery; this document does not authorize or
sequence an application mutation.

## Construction And Safe Context

`weavelit-server-observability` constructs a `System` record with result
`Failure` and classification `dependency.audit-log-unavailable`. Its variable
context is limited to two already validated values:

- the lowercase kebab-case Log Module identifier for the assigned destination;
  and
- the closed typed Audit classification for the affected operation.

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

## Terminal Recovery Reporting

The operational Audit terminal recovery coordinator uses the same safe System
record producer for list, opaque import, assignment resolution, exact delivery,
and Application Database acknowledgement failures. Recovery generates fresh
record identity and event time internally, uses the fixed correlation
identifier `audit-terminal-recovery`, and supplies
`internal.log-policy.changed` as the affected typed Audit classification. It
supplies the validated assigned Log Module identifier when resolution reached
one and the fixed safe identifier `unresolved` otherwise.

Recovery reporting has no client-error mapping. Each observed failure attempts
one System Log delivery and then returns the recovery sequence state. Failure
in record construction or System delivery is absorbed, the Server process
continues, and read and authentication paths remain available. An empty,
healthy activation produces no unavailable record.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Audit Log Design](../audit/audit-log-design.md)
- [Authorization-Denial System Log Record](authorization-denial-record-design.md)
- [Glossary](../../glossary.md)
- [Testing and Validation Policy](../../testing.md)
