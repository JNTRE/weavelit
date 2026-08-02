# Log Module Design

This document defines the shared design for Server-side
**[Log Modules](../glossary.md#applications-and-interfaces)**. It does not
define a destination-specific storage, delivery, retention, backup, or
migration implementation.

## Contract And Delivery Boundary

`weavelit-server-log` is the Server-owned shared contract and dispatch crate.
It defines a bounded typed record envelope with distinct System and Audit
variants, declared module capabilities, trusted registration and factory
inputs, durable-delivery acknowledgement, and payload-free typed errors. The
common envelope includes a Server-generated opaque record identifier, event
time, result, and correlation identifier. It contains no SQLite, filesystem,
Application Database, client-wire serialization, query, retention, backup,
recovery, purge, or remote-credential behavior.

Its compiled-in catalog validates each registration before invoking its factory
with trusted Server context. A configured destination accepts only a complete
immutable `CompleteLogRecord`; no public delivery operation accepts a raw
source payload or a caller-created record identifier. The destination must
acknowledge the same identifier and type synchronously after durable commit or
an exact prior-record match. A capability mismatch, malformed registration,
unavailable destination, or conflicting replay returns a stable payload-free
error.

Server Audit constructs and pre-redacts Audit records; Server Observability
constructs and pre-redacts System records, including Init and Restore completion
results. A Log Module accepts only these complete typed records. It may validate
its declared capability and persist or deliver a record, but it must not redact,
enrich, reinterpret, or access Application Database state.

Delivery is synchronous and succeeds only when the assigned destination has
durably committed the complete record or confirms an exact existing record with
the same type and record identifier. Before an Init or Restore application-state
commit, the Server creates the opaque identifier and persists it with immutable
completion-record fields in the post-commit obligation. Reconciliation retries
the identical record until it receives a durable acknowledgement. A matching
identifier with different content is an integrity failure. This provides
at-least-once attempts and one persisted completion record per identifier for
the MVP SQLite destination; it does not claim distributed exactly-once delivery
or select a fan-out policy.

## MVP SQLite Destination

`weavelit-module-log-sqlite` is the compiled-in MVP destination implementation.
It exclusively owns the Server-derived deployment-local destination at
`WEAVELIT_STATE_ROOT/log.sqlite3`, including recognized SQLite sidecars,
deployment binding, schema and migration ledger, connection, transactions,
health checks, locking behavior, and redacted error mapping. No client contract
accepts a path, filename, URI, or connection string for this destination.

The destination stores System and Audit records separately within its own
database. It must not depend on or reuse an Application Database crate, file,
schema, connection, configuration, or resource. It may use the same
workspace-pinned `rusqlite` package after dependency review. The Server
preflights the destination before an Init or Restore application-state commit,
keeps it for the process lifetime, and reopens and validates it during startup
or post-commit reconciliation. Restore imports Module configuration and
assignments, never destination data.

This MVP defines one local SQLite destination rather than Server-issued
multiple destination instances. Destination backup, recovery, retention, purge,
and automatic cleanup of valid preflight artifacts remain outside this design.

## Init And Restore Configuration

During **[Init](../glossary.md#states-and-requests)**, the person completing the
workflow uses an Init-capable
**[Pre-Operational Surface](../glossary.md#applications-and-interfaces)**
provided by a Client Module to select, configure, and activate one or more Log
Modules before assigning destinations for the two log types.

The **[Application Database](../glossary.md#applications-and-interfaces)** is
selected and configured by the shared lifecycle contract before Init accepts
Log Module configuration. Selecting the same underlying technology for an
Application Database and a Log Module does not reuse Weavelit-owned persistence
logic or implementation crates, configuration, database file, schema,
connection, or other resources. They may use the same workspace-pinned
third-party dependency, such as `rusqlite`, without sharing persistence
behavior. A Log Module may instead deliver records to a non-database
destination, such as email, an API endpoint, or Checkmk; its destination type
does not affect Application Database behavior.

The Init contract collects configuration in this order:

1. Select and configure a Log Module for **[System Logs](../glossary.md#applications-and-interfaces)**.
2. Assign that configured Log Module to receive System Logs.
3. Select and configure a Log Module for **[Audit Logs](../glossary.md#applications-and-interfaces)**.
4. Assign that configured Log Module to receive Audit Logs.

The person completing Init may select the same configured Log Module for both
assignments. Every Init-capable Client Module submits the same module
configuration and two explicit assignments to the same Server-owned
validation; no client defines an alternative Log Module initialization path.

Init rejects an absent, disabled, unconfigured, or incompatible assignment. It
also rejects an assignment unless its configured Log Module can durably record
the assigned log type. After Init commits application state, it durably records
the Init completion result through the committed System Log assignment before
the deployment is sealed. Init remains incomplete, and the Server does not begin
normal operation, until both assignments are valid and the completion result is
durable.

During **[Restore](../glossary.md#states-and-requests)**, the Server imports Log
Module configurations, enabled state, assignments, and protected credentials
from the validated Application Database backup. It does not import System Log
or Audit Log destination data. Every referenced Log Module must be compiled
into the replacement Server, and every restored configuration and assignment
must satisfy the same Server-owned validation used during normal administration.

Restore validates both restored assignments and does not seal the replacement
deployment until the restored System Log assignment durably records the required
Restore result without recovery secrets or backup contents. A failure remains
non-operational and follows the post-commit reconciliation rules in the
[Server Restore Design](../server/lifecycle/restore/restore-design.md). A restored Log Module
never reads backup contents or Application Database state directly.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Open Questions](../open-questions.md)
- [Glossary](../glossary.md)
- [Application Database Design](../server/database/application-database-design.md)
- [Server Restore Design](../server/lifecycle/restore/restore-design.md)
