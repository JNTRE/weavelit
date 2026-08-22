# Audit Terminal Binding Retention And Supersession

## Status

Accepted.

## Context

A normal-operation **[Audit Log](../glossary.md#applications-and-interfaces)**
terminal record may remain pending after its consequential mutation commits.
Its immutable recovery projection names the exact destination configuration
identity and binding version that accepted the corresponding Attempt. A later
configuration change cannot safely rewrite that obligation for a new
destination, claim that a different record proves delivery, or let one
permanently lost destination block every later accountable mutation forever.

The approved policy was recorded in
[Issue #150 comment 5336031944](https://github.com/JNTRE/weavelit/issues/150#issuecomment-5336031944).
This record preserves why ordinary changes retain old bindings and why the one
exception is a visible integrity degradation rather than delivery proof.

## Decision

Ordinary Log Module configuration and assignment changes retain every prior
binding identity, version, and resolvable destination handle while any terminal
obligation still references it. Exact replay continues only through that
retained binding. Configuration generations, change sequencing, destination
retention implementation, and the recovery user interface belong to the future
Administration Plane configuration work.

An Administrator may supersede only the exact oldest valid active terminal
obligation after repair of its retained binding has failed and that destination
is permanently unavailable. The action requires fresh password
reauthentication for the exact current session, fresh TOTP verification when
the account is enrolled, explicit confirmation of the named obligation and
replacement, and successful Audit preflight of the replacement binding. A
boolean request flag is not authorization or confirmation evidence.

Supersession appends a bounded disposition with the original obligation
identity, original binding, fixed reason
`destination_permanently_unavailable`, replacement binding, completeness
`degraded`, and original state `retained_pending_late_delivery`. It never
rewrites, removes, or acknowledges the original. Its trusted transaction input
also retains the exact validated original opaque projection. Before appending,
storage must compare the stored identity and projection bytes with that exact
original and compare the stored retained binding with the disposition. A
same-identity projection or binding mismatch fails without mutation. The
replacement assignment, append-only disposition, and new supersession Audit
terminal recovery obligation share one transaction boundary. The new Audit action is
`dependency.audit-terminal.superseded` with action
`supersede-terminal-delivery`; it records the degraded exception and is not a
Correction or evidence that the original reached any destination.

The original remains eligible for exact late delivery through its retained
binding. Only that exact destination acknowledgement may acknowledge the
original. Delivery of another record to the replacement, a System Log, Restore,
or any lifecycle completion record cannot substitute for it. Until exact late
delivery completes, the deployment exposes degraded Audit completeness.

## Consequences

Destination changes may retain old resources longer than their current
configuration lifetime, and operators must be able to distinguish active
recovery from retained late-delivery obligations. A permanently unavailable
destination no longer blocks the active sequence after the constrained
supersession transaction, but the deployment truthfully reports incomplete
Audit evidence.

The Application Database needs append-only disposition storage, active and
late-delivery ordering, and exact acknowledgement checks. Runtime drain,
concrete SQLite persistence, and the Administration Plane route and user
experience remain separate follow-on work. Reversing this decision would
require a new accepted policy that preserves immutable accountability and does
not convert replacement delivery into false proof.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Log Module Design](log-module-design.md)
- [Server Audit Log Design](../server/audit/audit-log-design.md)
- [Application Database Design](../server/database/application-database-design.md)
- [Server API Contract](../server/api/api-contract-design.md)
- [Testing and Validation Policy](../testing.md)
