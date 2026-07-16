# SQLite Application Database Design

This document defines the MVP SQLite implementation of the Server's internal
**[Application Database](../../../glossary.md#applications-and-interfaces)**.
It implements the shared backend contract in the
[Application Database Design](../application-database-design.md).

## Crate And Driver

`weavelit-server-database-sqlite` is the dedicated SQLite implementation crate.
It depends on `weavelit-server-database` and owns SQLite connectivity, explicit
SQL, schema migrations, transaction behavior, connection health, and
SQLite-specific error mapping. It does not own Server lifecycle decisions,
shared contract types, or **[Log Module](../../../glossary.md#applications-and-interfaces)**
destinations.

The crate uses `rusqlite` with its `bundled` feature. The Server workspace
centralizes and locks the selected dependency version. Bundling provides a
known SQLite release and consistent behavior across development, CI, packaging,
and deployment without relying on a host SQLite shared library.

The backend uses explicit Weavelit-owned SQL and does not use an ORM. It does
not enable runtime SQLite extension loading or execute user-supplied SQL.

## Migrations And Transactions

The crate embeds explicit, forward-only SQL migrations. Files use this immutable
name format:

```text
NNNN_<action>_<subject>.sql
```

`NNNN` is a unique, zero-padded ascending sequence. Names use lowercase
`snake_case`; `action` is one of `create`, `add`, `alter`, `migrate`, or
`remove`; and `subject` describes the durable schema or data intent rather than
SQL mechanics. For example:

```text
0001_create_application_state.sql
0002_add_listening_address.sql
0003_migrate_group_grants.sql
```

Released migrations are never edited, renamed, reordered, or reused. A
correction is a new migration. A destructive or lossy `remove` migration
requires a separately documented backup and compatibility decision.

The SQLite database contains an Application Database-owned migration ledger of
applied identifiers and checksums. On opening the backend, it validates ledger
entries and applies pending migrations in ascending order. Each migration and
its ledger entry run in one SQLite transaction. An unknown, missing, or
checksum-mismatched applied migration is an integrity or compatibility failure:
the backend changes nothing and refuses to report readiness.

Each shared Application Database contract write is one SQLite transaction unless
the contract explicitly defines a broader atomic workflow. A failed migration
or write rolls back completely.

## Connection And Health Behavior

Each SQLite backend instance owns one SQLite connection for its lifetime and
serializes access to it. The MVP does not use a connection pool. This is a
SQLite-specific choice, not a constraint on a future Application Database
backend.

The backend enables foreign-key enforcement, write-ahead logging, and a bounded
busy timeout. It applies and validates migrations before it is ready. A real,
lightweight database query verifies connection health.

Failure to open, migrate, validate, or query the Application Database prevents
safe startup. The backend reports a typed, redacted contract error rather than
a raw driver or filesystem error.

## Errors And Test Evidence

SQLite-specific errors are private implementation details. Before returning to
the Server, the backend maps them to the shared contract's storage-neutral
errors. Returned errors and ordinary diagnostics never include raw SQLite or
operating-system messages, SQL, filesystem paths, connection settings, or
secrets. Diagnostics may include deliberately redacted structured context such
as a migration identifier.

Integration tests use a new `tempfile` temporary directory and a real SQLite
database file for each test. Tests reopen the file to verify persistence and
allow the directory to remove database and WAL sidecar files automatically.
They cover successful and idempotent migrations, restart persistence, migration
and write rollback, invalid configuration, unavailable storage, incompatible
migration history, and error and diagnostic redaction.

The MVP scope does not defer quality obligations: every selected behavior has
its required security, diagnostics, safe failure, and automated test evidence
from the outset.

## Related Documents

- [Application Database Design](../application-database-design.md)
- [Core Statements](../../../core-statements.md)
- [Security Model](../../../security-model.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
