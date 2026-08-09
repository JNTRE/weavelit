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

The initial registry contains `0001_create_migration_ledger.sql`,
`0002_create_lifecycle_state.sql`, and `0003_create_application_state.sql`. Each
entry has a one-based sequence, the filename without `.sql` as its identifier,
and SQL embedded through `include_str!`. `sha2 = "=0.11.0"` computes a 32-byte
SHA-256 digest directly over the exact embedded UTF-8 file bytes with default
features disabled. The ledger stores the digest as a 32-byte BLOB and rejects
updates or deletes.

Migration application repeats one `BEGIN IMMEDIATE` transaction at a time. The
transaction validates that ledger rows form the exact contiguous prefix of the
embedded registry, then applies at most the next SQL migration and inserts its
ledger row before commit. The bootstrap transaction creates the ledger and
records itself atomically. An absent ledger is eligible only when no
Application Database-owned table exists; otherwise startup fails with
`IntegrityFailure` rather than recreating history.

After ledger validation and before every pending migration, the backend builds
the expected application-owned schema for the already-applied migration prefix
in an isolated in-memory SQLite connection. It compares that schema with every
installed table, index, and trigger whose name or owning table begins with
`weavelit_`. A dropped or altered table or constraint, missing production
trigger, or added trigger or index fails with `IntegrityFailure` before another
migration can change the database. The complete schema receives the same check
before readiness. Schema SQL remains private and is never returned or logged.

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

## Application State Schema

`0003_create_application_state.sql` creates one `STRICT` table for each
normalized state type defined by the shared
[Application Database Design](../application-database-design.md):
`weavelit_configuration`, `weavelit_protected_secret`, `weavelit_account`,
`weavelit_password_verifier`, `weavelit_group`, `weavelit_group_membership`,
`weavelit_group_grant`, `weavelit_mfa_factor`, `weavelit_service_connection`,
`weavelit_recovery_public_key`, `weavelit_log_module_configuration`,
`weavelit_log_module_setting`, `weavelit_log_assignment`, and
`weavelit_completion_obligation`. The migration creates no session table, no Log
Module destination-data or log-record table, and no Log Module credential
column, so excluded data has no place to be written.

Each table restates the contract's bounds as SQL `CHECK` constraints so durable
data cannot drift from the typed contract: 16-byte nonzero identifier BLOBs,
byte-length ranges measured with `length(CAST(value AS BLOB))`, `0` or `1`
boolean integers, and closed sets for the grant kind, log type, and workflow
discriminator. Foreign keys bind verifiers, memberships, grants, MFA factors,
log settings, and log assignments to their owning rows. The recovery public key
and the completion obligation each use a singleton row. Protected values and
protected credentials are stored as opaque BLOBs; the backend never inspects,
derives, or transforms them.

Group grants encode their kind in `grant_kind` and their subject in
`grant_value`. A `server_administration` grant carries no subject and requires
an empty value; every other kind requires a bounded non-empty value. Log module
assignment stores the log type as its own primary key, so SQLite itself allows
at most one row for each of the System Log and the Audit Log.

Reads decode every row through the shared contract's checked constructors and
rebuild the aggregate through `ApplicationState`. Any invalid identifier,
out-of-bounds text, unknown discriminator, missing singleton, duplicate row,
dangling reference, or disabled log assignment returns `IntegrityFailure`
without returning the offending value.

## Checkpoint Transactions

`SqliteDatabase` implements the complete `ApplicationDatabase` contract after
the read and mutation paths are available. Trait inspection delegates to the
inherent read-only operation. Checkpoint creation begins an immediate SQLite
transaction, loads and validates the bounded lifecycle state under that write
lock, and commits only the allowed result. Separate backend instances therefore
serialize and recheck stale requests before any write.

Creation inserts a pending singleton row only when inspection returns
`Uninitialized`. A pending row returns `InvalidState` even when the requested
checkpoint is identical, and initialized state returns `AlreadyInitialized`.
The backend exposes no reconciliation or discard operation: a pending row is
retained for fail-closed lifecycle-interruption classification. Malformed state
returns `IntegrityFailure` before mutation. SQL parameters bind identifiers and
metadata as BLOBs and workflow as a fixed internal string. Driver payloads,
SQL, paths, identifiers, and metadata are never included in returned errors or
ordinary diagnostics.

Checkpoint replacement runs in one `BEGIN IMMEDIATE` transaction. Under that
write lock it re-inspects lifecycle state against the expected deployment
identifier, requires the persisted pending checkpoint to equal the supplied
checkpoint exactly, writes every state row, inserts the unacknowledged
completion obligation, and updates the singleton lifecycle row to initialized
with no workflow or checkpoint metadata. The whole replacement therefore
commits or rolls back as one unit, and the second attempt sees initialized
state and returns `AlreadyInitialized`.

Acknowledgement runs in its own immediate transaction. It requires initialized
state for the expected deployment and updates the obligation row only while it
is unacknowledged and its persisted record identifier matches; otherwise it
returns `InvalidState` without writing. Loading initialized state runs one read
transaction, decodes every state table, and reports the outstanding
acknowledgement flag alongside the aggregate.

Real-SQLite tests use separate stale backend instances to prove serialized
rechecking and test-only table triggers to force insert failures. The trigger
failures roll back fully and remain unchanged after reopen without
adding a production failure-injection mechanism.

Each shared Application Database contract write is one SQLite transaction unless
the contract explicitly defines a broader atomic workflow. A failed migration
or write rolls back completely.

## Connection And Health Behavior

Each SQLite backend instance owns one SQLite connection for its lifetime and
serializes access to it. The MVP does not use a connection pool. This is a
SQLite-specific choice, not a constraint on a future Application Database
backend.

The backend exposes a trusted-path `SqliteDatabase::open` constructor and the
complete `ApplicationDatabase` implementation documented above. It does not
expose the raw connection, arbitrary query execution, or URI or
connection-string configuration.

The lifecycle crate supplies the exact code-defined
`WEAVELIT_STATE_ROOT/application.sqlite3` location. The SQLite backend schema
exposes no path or filename field to a client. The Server and SQLite backend
exclusively create, place, reopen, and manage that file and the code-owned
`application.sqlite3-journal`, `application.sqlite3-wal`, and
`application.sqlite3-shm` recovery sidecars. The lifecycle root inventory
recognizes those names but never deletes the sidecars; SQLite owns their
validation, recovery, and removal.

The backend opens that location for reading, writing, and creation with
`SQLITE_OPEN_NO_MUTEX` and `SQLITE_OPEN_NOFOLLOW`. It deliberately omits
`SQLITE_OPEN_URI`, so query-like text in a supplied path is treated as a literal
filename and cannot select SQLite URI behavior. `SQLITE_OPEN_NOFOLLOW` rejects a
symbolic link in the supplied database path. The lifecycle boundary must supply
a symlink-free path beneath the protected Server state directory; protection of
those parent directories remains a Server deployment responsibility.

Before retained inspection opens SQLite, the backend checks the trusted derived
WAL path through non-mutating filesystem metadata. An existing WAL returns an
uninspectable retained-state result to the lifecycle boundary, which classifies
it as the generic `lifecycle_interrupted` / `operator_redeploy_required` action.
The backend does not open, copy, inspect, recover, checkpoint, clean up, or
otherwise modify the original database, WAL, or shared-memory artifacts in that
case.

does not configure pragmas or WAL, apply migrations, create files or sidecars,
Only when no WAL is present does retained inspection open a private absolute
`file:` URI for the same trusted path. It percent-encodes every
non-unreserved path byte while retaining separators and appends the fixed
`?immutable=1` query. This URI is opened with exactly read-only,
`SQLITE_OPEN_NO_MUTEX`, `SQLITE_OPEN_NOFOLLOW`, and `SQLITE_OPEN_URI` access;
the URI flag applies only to retained inspection, not ordinary database opens.
It does not configure pragmas or WAL, apply migrations, create files or
sidecars, checkpoint, recover, clean up, or write. This safe inspection may
establish the exact Init or Restore interruption action for retained state
without WAL ambiguity.

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

Application-state tests additionally cover a full round trip of every state type
across reopen, one-time checkpoint replacement, mismatched-checkpoint and
mismatched-deployment rejection without writing state, complete rollback of a
partially written replacement, malformed persisted state, one-time obligation
acknowledgement, and an explicit assertion over the installed `weavelit_*`
tables and columns that no session, log-record, or Log Module credential
storage exists.

The MVP scope does not defer quality obligations: every selected behavior has
its required security, diagnostics, safe failure, and automated test evidence
from the outset.

## Related Documents

- [Application Database Design](../application-database-design.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
