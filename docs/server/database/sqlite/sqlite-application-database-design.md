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

The crate declares `rusqlite = "=0.40.1"` with default features disabled and
only the `bundled` feature enabled. It remains in the crate manifest as the
initial single consumer. If a second Server crate requires `rusqlite`, that
change promotes its shared source, version, and security baseline to
`server/Cargo.toml`'s `[workspace.dependencies]`. `server/Cargo.lock` records the
resolved version. Bundling provides a known SQLite release and consistent
behavior across development, CI, packaging, and deployment without relying on
a host SQLite shared library.

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

The initial registry contains `0001_create_migration_ledger.sql` and
`0002_create_lifecycle_state.sql`. Each entry has a one-based sequence, the
filename without `.sql` as its identifier, and SQL embedded through
`include_str!`. `sha2 = "=0.11.0"` computes a 32-byte SHA-256 digest directly
over the exact embedded UTF-8 file bytes with default features disabled. The
ledger stores the digest as a 32-byte BLOB and rejects updates or deletes.

Migration application repeats one `BEGIN IMMEDIATE` transaction at a time. The
transaction validates that ledger rows form the exact contiguous prefix of the
embedded registry, then applies at most the next SQL migration and inserts its
ledger row before commit. The bootstrap transaction creates the ledger and
records itself atomically. An absent ledger is eligible only when no
Application Database-owned table exists; otherwise startup fails with
`IntegrityFailure` rather than recreating history.

The lifecycle schema uses one singleton row. No row represents uninitialized
state. A row always contains a 16-byte deployment identifier and represents
either pending state with an `init` or `restore` discriminator and at most 4 KiB
of opaque checkpoint metadata, or initialized state with no pending workflow or
checkpoint metadata. This schema does not expose production state operations;
inspection and mutation remain owned by their later contract work.

State inspection is exposed first as the inherent read-only
`SqliteDatabase::inspect` operation. It performs one query ordered by the
singleton key and limited to two rows. Zero rows returns `Uninitialized`, one
row is decoded, and two rows fails with `IntegrityFailure`. The complete
`ApplicationDatabase` implementation is added only when all mutation methods
are available.

Inspection first requires a valid 16-byte nonzero persisted deployment
identifier. A valid identifier different from the trusted expected identifier
returns `DeploymentMismatch` before state fields are interpreted or returned.
For a matching identifier, `pending` requires `init` or `restore` and present
metadata within the 4 KiB bound; `initialized` requires both workflow and
metadata to be absent. Every malformed or contradictory persisted combination
returns `IntegrityFailure`. Inspection emits no diagnostics and returns only
payload-free storage-neutral errors.

Each shared Application Database contract write is one SQLite transaction unless
the contract explicitly defines a broader atomic workflow. A failed migration
or write rolls back completely.

## Connection And Health Behavior

Each SQLite backend instance owns one SQLite connection for its lifetime and
serializes access to it. The MVP does not use a connection pool. This is a
SQLite-specific choice, not a constraint on a future Application Database
backend.

The connection baseline exposes only a trusted-path `SqliteDatabase::open`
constructor. It does not expose the raw connection, arbitrary query execution,
URI or connection-string configuration, or an `ApplicationDatabase`
implementation. Migrations and durable contract behavior are added with their
own schema and state work.

The lifecycle crate supplies a code-defined database location under the
protected Server state directory. The SQLite backend schema exposes no path or
filename field to a client. The Server and SQLite backend exclusively create,
place, reopen, and manage the database file and its journal or write-ahead-log
sidecar files.

The backend opens that location for reading, writing, and creation with
`SQLITE_OPEN_NO_MUTEX` and `SQLITE_OPEN_NOFOLLOW`. It deliberately omits
`SQLITE_OPEN_URI`, so query-like text in a supplied path is treated as a literal
filename and cannot select SQLite URI behavior. `SQLITE_OPEN_NOFOLLOW` rejects a
symbolic link in the supplied database path. The lifecycle boundary must supply
a symlink-free path beneath the protected Server state directory; protection of
those parent directories remains a Server deployment responsibility.

Before returning a connection, the backend performs and verifies this fixed
configuration sequence:

1. Enable foreign-key enforcement and verify `foreign_keys` is `1`.
2. Request write-ahead logging and verify the returned journal mode is `wal`.
3. Set a five-second busy timeout through the driver API and verify
   `busy_timeout` is `5000` milliseconds.
4. Execute the fixed internal `SELECT 1` health query and require the integer
   result `1`.

The fixed query is not caller-supplied SQL and does not assume that migrations
or application schema already exist. Later migration work runs only after this
connection baseline reports readiness.

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

The connection baseline maps SQLite `DatabaseCorrupt` and `NotADatabase` codes
to `IntegrityFailure`. Busy, locked, read-only, permission, input/output,
disk-full, cannot-open, and locking-protocol codes map to `Unavailable`. A path
that the driver cannot represent maps to `ConfigurationInvalid`. An unexpected
failure in fixed internal configuration or health logic maps to
`IntegrityFailure`, and a completed setting whose verified value does not match
the required value maps to `ConfigurationInvalid`. The mapper discards driver
payloads immediately and emits no ordinary diagnostic containing them.

Integration tests use the test-only `tempfile = "=3.27.0"` dependency to create
a new temporary directory and real SQLite database file for each test. Tests
reopen the file to verify persistence and allow the directory to remove database
and WAL sidecar files automatically. They cover successful and idempotent
migrations, restart persistence, migration and write rollback, invalid
configuration, unavailable storage, incompatible migration history, and error
and diagnostic redaction.

The MVP scope does not defer quality obligations: every selected behavior has
its required security, diagnostics, safe failure, and automated test evidence
from the outset.

## Related Documents

- [Application Database Design](../application-database-design.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
