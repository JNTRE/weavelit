# Log Module Design

This document defines the shared design for Server-side
**[Log Modules](../glossary.md#applications-and-interfaces)**. It does not
define a destination-specific storage or delivery implementation. It defines
the recovery and retention-policy boundaries that every destination must
preserve.

## Contract And Delivery Boundary

`weavelit-server-log` is the Server-owned shared contract and dispatch crate.
It defines a bounded typed record envelope with distinct System and Audit
variants, declared module capabilities, trusted registration and factory
inputs, durable-delivery acknowledgement, and payload-free typed errors. The
common envelope includes a Server-generated opaque record identifier, event
time, result, and correlation identifier. It contains no SQLite, filesystem,
Application Database, client-wire serialization, query, retention, backup,
recovery, purge, or remote-credential behavior.

The contract enforces UTF-8 byte limits before it constructs a complete record:
the correlation identifier is at most 64 bytes; System classification and
detail are at most 128 bytes and 4 KiB; and Audit principal, action, target,
and detail are at most 256 bytes, 128 bytes, 1 KiB, and 4 KiB. The correlation
identifier plus every body field is at most 8 KiB. Empty and oversized values
are rejected without truncation, hashing, raw source payload retention, or a
replacement record. Audit and Observability are the only producers of these
pre-redacted bounded summaries; a logging-required workflow fails if it cannot
construct one, and a destination receives no unbounded or partial record.

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
completion-record fields in the post-commit obligation. During a valid
uninterrupted workflow run, the Server delivers that identical record until it
receives a durable acknowledgement. An interruption before acknowledgement
leaves the workflow non-operational; the lifecycle does not retry delivery,
construct a replacement record, or seal on restart. A matching identifier with
different content is an integrity failure. This provides at-least-once attempts
and one persisted completion record per identifier for the MVP SQLite
destination; it does not claim distributed exactly-once delivery or select a
fan-out policy.

## MVP SQLite Destination

`weavelit-module-log-sqlite` is the compiled-in MVP destination implementation.
It exclusively owns the Server-derived deployment-local destination at
`WEAVELIT_STATE_ROOT/log.sqlite3`, including recognized SQLite sidecars,
deployment binding, schema and migration ledger, connection, transactions,
health checks, locking behavior, and redacted error mapping. No client contract
accepts a path, filename, URI, or connection string for this destination.

The runtime constructs one validated SQLite registration in its compiled-in Log
Module catalog after lifecycle startup classification and retains that catalog
for the process lifetime. Catalog construction does not invoke the destination
factory. Until a later Server-owned configuration and assignment flow selects
the module, startup neither opens nor delivers to the destination.

The destination stores System and Audit records separately within its own
database. It must not depend on or reuse an Application Database crate, file,
schema, connection, configuration, or resource. It may use the same
workspace-pinned `rusqlite` package after dependency review. The Server
preflights the destination before an Init or Restore application-state commit,
keeps it for the process lifetime, and validates it during startup without
post-commit reconciliation delivery. Restore imports Module configuration and
assignments, never destination data.

This MVP defines one local SQLite destination rather than Server-issued
multiple destination instances. The recovery and capacity policy below defines
requirements for future destination implementation work; it does not add
backup, recovery, retention, purge, or capacity behavior to the completed
append-only MVP SQLite implementation.

The SQLite destination derives its fixed `log.sqlite3` filename only from the
trusted local root supplied to its factory; it does not inspect an environment
variable or accept a client path. It opens the destination without following a
database-file symlink, uses SQLite write-ahead logging with full synchronous
commit behavior, and serializes its owned connection. Transient SQLite sidecars
remain part of this destination's owned resource set when SQLite creates them.
It stores the complete fields of System and Audit records in separate typed
tables rather than exposing a serialized record blob.

Freshness requires the absence of every recognized artifact: `log.sqlite3`,
`log.sqlite3-journal`, `log.sqlite3-wal`, and `log.sqlite3-shm`. When the main
database is absent but any recognized sidecar exists, the destination must fail
closed with an integrity failure before reserving or opening the main database,
configuring SQLite, or validating the binding, ledger, or schema. It must not
alter an orphan sidecar. When the main database exists, its binding, ordered
checksummed migration ledger, expected ledger-prefix schema, and SQLite
sidecars retain their existing validation and recovery behavior. A missing,
empty, malformed, duplicate, mismatched, unknown, reordered, changed, or
schema-mutated artifact fails closed without altering the destination. Fresh
bootstrap atomically creates migration 1, its ledger entry, and the matching
binding before later migrations may run. Opening, health, lock, and delivery
failures map only to the shared payload-free destination errors; they do not
disclose paths, SQL, record contents, or secrets.

SQLite stores the bounded record fields with byte-based `CAST(... AS BLOB)`
constraints and the same aggregate 8 KiB maximum. A migration that adds these
constraints rebuilds its tables transactionally and copies existing records
only when they satisfy the new schema. An existing oversized row makes the
migration fail closed without dropping, altering, or replacing any destination
record; this MVP defines no data-recovery exception for that incompatibility.

## Destination Recovery And Retirement

Each destination owns its protection, snapshot or backup, migration,
compatibility, recovery, and retirement behavior. A replacement destination
must create or validate destination lineage. An unknown, corrupt, mismatched,
or incompatible artifact must fail closed with a stable, redacted error; a
destination must never automatically reset, overwrite, or delete it.

SQLite destination protection and copying must use a destination-owned,
SQLite-consistent snapshot procedure. Copying a lone WAL-mode database file is
not a valid snapshot or recovery procedure. A future remote destination must
choose either source-bound replacement lineage or validated shared continuity
with exact record-identifier replay.

## Destination Retention And Capacity

Retention and purge are destination-owned and Administrator-selected only when
the destination declares them relevant. A destination, including an email
destination, may declare retention unsupported. The Server does not provide an
automatic Server-wide purge or arbitrary global retention default.

SQLite capacity protection is opt-in and disabled by default. When an
authorized Administrator enables it, the SQLite destination validates and uses
a module-specific page budget, filesystem and WAL reserve, and System Log purge
rule and target. It must inspect its actual runtime page size and page ceiling
and reject an invalid budget. Before reaching its hard budget, it may perform
only the configured destination-owned purge and checkpoint work. It must not
perform automatic `VACUUM`.

At the hard budget or on SQLite `FULL`, the SQLite destination must stop durable
delivery with a stable, payload-free unavailable error. It must not delete
additional records unless the configured policy permits that deletion. System
Logs may be purged only through the configured SQLite policy. Audit Logs must
never be automatically purged; any Audit Log retention or deletion capability
requires a future explicit authorized decision that includes a hold policy.

A future Server administration contract owns authorization, confirmation,
policy, run, and status APIs and the Audit Logs for policy changes and purge
start, failure, and completion. This policy does not introduce purge behavior
to the completed append-only MVP SQLite implementation.

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

During **[Restore](../glossary.md#states-and-requests)**, the Server imports
only non-secret Log Module configurations, enabled state, and assignments from
the validated Application Database backup. It does not import System Log or
Audit Log destination data or authentication or connection credentials. A
restored remote destination remains unusable until an authorized Administrator
re-enters its credentials through an
**[Administration Plane](../glossary.md#applications-and-interfaces)**. Every
referenced Log Module must be compiled into the replacement Server, and every
restored configuration and assignment must satisfy the same Server-owned
validation used during normal administration.

Restore validates both restored assignments and does not seal the replacement
deployment until the restored System Log assignment durably records the required
Restore result without recovery secrets or backup contents. A failure remains
non-operational and follows the retained-state interruption boundary in the
[Server Restore Design](../server/lifecycle/restore/restore-design.md). A restored
Log Module never reads backup contents or Application Database state directly.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Open Questions](../open-questions.md)
- [Glossary](../glossary.md)
- [Application Database Design](../server/database/application-database-design.md)
- [Server Restore Design](../server/lifecycle/restore/restore-design.md)
