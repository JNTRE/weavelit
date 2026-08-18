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

The registry contains `0001_create_migration_ledger.sql`,
`0002_create_lifecycle_state.sql`, `0003_create_application_state.sql`,
`0004_create_session_store.sql`, and
`0005_add_mfa_policy_and_replay_watermark.sql`,
`0006_add_lifecycle_reconciliation.sql`, and
`0007_add_audit_references.sql`. Each
entry has a one-based sequence, the filename without `.sql` as its identifier,
and SQL embedded through `include_str!`. `sha2 = "=0.11.0"` computes a 32-byte
SHA-256 digest directly over the exact embedded UTF-8 file bytes with default
features disabled. The ledger stores the digest as a 32-byte BLOB and rejects
updates or deletes. An applied migration is immutable: its embedded bytes can
never change, because the recorded checksum would no longer match and the
backend would refuse to open. A schema change is therefore always a new
migration.

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
column, so excluded data has no place to be written. Live sessions are created
by a separate migration described in
[Live Session Schema](#live-session-schema) and are not part of this aggregate.

Each table restates the contract's bounds as SQL `CHECK` constraints so durable
data cannot drift from the typed contract: 16-byte nonzero identifier BLOBs,
byte-length ranges measured with `length(CAST(value AS BLOB))`, `0` or `1`
boolean integers, and closed sets for the grant kind, log type, and workflow
discriminator. Foreign keys bind verifiers, memberships, grants, MFA factors,
log settings, and log assignments to their owning rows. The recovery public key
and the completion obligation each use a singleton row. Protected values and
protected credentials are stored as opaque BLOBs; the backend never inspects,
derives, or transforms them.

`0007_add_audit_references.sql` creates separate `STRICT`
`weavelit_account_audit_reference` and
`weavelit_group_audit_reference` tables. Each owner column is both the primary
key and a foreign key to its typed owning table, so each existing account or
Group can carry at most one association and an orphan or wrong-kind owner is
rejected. Each table checks the exact nonzero `ar-` plus
32-lowercase-hexadecimal representation and has a named unique index on the
reference value. Reciprocal insert triggers reject reuse across the two entity
kinds, and per-table update triggers reject every association update. Future
entity types receive their own typed table only when their owning application
state exists. Names and state identifiers remain in their owning tables and
are never used as reference values.

The migration backfills every existing account and Group with a fresh
independent 16-byte SQLite `randomblob` value rendered canonically. This
migration-only backfill does not derive values from identifiers or names and
does not change the Application Database contract's ownership of runtime
generation. Both table creations, both backfills, and migration-ledger
insertion share the migration's single immediate transaction. A zero value,
per-table or cross-table collision, or later statement failure violates the
schema and rolls the entire migration back, preserving all prior entity fields
and the six-entry ledger prefix. State reads use the persistence decoder
supplied by lifecycle's selected database wrapper and rebuild typed account and
Group projections through the backend-neutral checked constructor; a missing,
extra, malformed, reused, orphaned, or wrongly associated reference is an
`IntegrityFailure`.

### Future Temporary-Credential Migration

The future migration for temporary account credentials must be forward-only and
transactional. It adds, rather than preserves, `must_change_password`, a
credential revision, and fixed 24-hour expiry metadata to the account-owned
schema. It stores only the Argon2 verifier and bounded metadata; plaintext
temporary passwords, response buffers, delivery content, and continuation
bearer values have no SQLite column. No password-change ticket is authorized
by this design; if a future design adds one, its exact state and storage
requirements must be approved separately.

Future account creation, reset, and password change use one `BEGIN IMMEDIATE`
compare-and-set transaction over the expected credential revision. The
transaction writes or replaces the verifier and temporary metadata, increments
or replaces the revision, and revokes target sessions where applicable. It
does not store Audit records or participate in Audit sequencing; post-commit
recovery remains with the Audit design and owning workflow. A
stale revision rolls back all credential and session writes and returns a
stable secret-free conflict. The migration and transaction design do not
create a retrieval endpoint and do not change Init semantics. Refer to
[Authentication Design](../../authentication/authentication-design.md#future-account-credential-issuance)
for the approved expiry, revision, session, and reauthentication policy.

## Live Session Schema

`0004_create_session_store.sql` creates the single `STRICT` table
`weavelit_session`, which implements the shared contract's `SessionStore`. The
table holds `token_hash` as the primary key, `csrf_hash`, `account_id`,
`client_module`, `issued_at_milliseconds`, `last_seen_at_milliseconds`, and
`absolute_expires_at_milliseconds`, and nothing else. It carries no foreign key
into restorable state, because a live session must never bind the durability of
operational data to an aggregate a Restore replaces.

The digest columns `CHECK` that a value is exactly 32 bytes and is not
`zeroblob(32)`, so a plaintext token or CSRF value, whose encoded form is 43
characters, cannot be stored even by a direct statement.
`absolute_expires_at_milliseconds` is constrained to equal
`issued_at_milliseconds + 43200000`, and a `BEFORE UPDATE` trigger aborts any
statement that changes the token digest, the account, the Client Module, the
issue instant, or the absolute expiry. A second trigger aborts any update that
moves recorded activity backwards. The absolute expiry is therefore fixed at
creation by the schema, not only by the calling code. An index on `account_id`
serves account-wide revocation.

Every store operation runs in its own `BEGIN IMMEDIATE` transaction, so
validation, activity update, CSRF rotation, expiry removal, revocation, and
purging are atomic. Validation locates a candidate row by indexed digest
equality and then confirms the stored digest against the presented digest in
constant time; the accept decision rests on that constant-time comparison rather
on the storage engine's own byte comparison. A row whose stored bytes cannot be
rebuilt into the contract's types is refused as `IntegrityFailure` rather than
accepted.

Inserting a session first deletes rows already expired at the new session's
issue instant, in the same transaction. The delete is bounded to a named batch
constant by selecting that many `token_hash` values in a subquery, because
`DELETE ... LIMIT` requires a SQLite build option this deployment does not
rely on. A delete failure is deliberately absorbed rather than propagated: the
insertion still commits, because the login's correctness does not depend on
removing unusable rows, and those rows are offered to the next insertion. An
adverse condition that also prevents the insertion still fails the operation
through the insert itself.

Completing a checkpoint deletes every session row inside the same transaction
that installs the replacement state, so a Restore's session invalidation commits
or rolls back with the replacement itself and no interruption can land between
the two.

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

## MFA Policy And Replay Watermark Schema

`0005_add_mfa_policy_and_replay_watermark.sql` adds the account column
`mfa_required`, a `0` or `1` integer defaulting to `0`, so an existing
deployment starts with no account required to present a second factor.

The same migration creates the `STRICT` table
`weavelit_mfa_replay_watermark`, which implements the shared contract's
`MfaStore`. It holds `factor_id` as the primary key and `accepted_step`, and
nothing else. The identifier column `CHECK`s the contract's 16-byte nonzero
identifier shape and `accepted_step` `CHECK`s a non-negative value.

The table sits beside the live session table rather than on
`weavelit_mfa_factor`, and carries no foreign key into restorable state, for
the reason given in the
[Authentication Design](../../authentication/authentication-design.md#replay-watermark):
an enrolled factor is part of the aggregate a Restore replaces, while the
highest time step that factor has been observed to use is live operational
state produced by this running deployment.

Accepting a time step reads the Module's enabled setting, advances the
watermark, and inserts the session the acceptance issues, inside one
`BEGIN IMMEDIATE` transaction. A disabled Module returns before anything is
written. The advance itself is one statement: an upsert whose update branch is
guarded by
`excluded.accepted_step > weavelit_mfa_replay_watermark.accepted_step`. The
comparison and the write are therefore the same statement, and no concurrent
presentation can observe the pre-update watermark and be accepted alongside the
first. A changed-row count of zero means the presented step did not advance the
watermark and the presentation is reported as a replay, with the transaction
rolled back so no session is issued. A `BEFORE UPDATE`
trigger additionally aborts any statement, including a direct one, that reuses
or rewinds an accepted step, so a spent code cannot be made usable again by
writing to the table. Enrolling a factor spans the same three tables in one
transaction for the same reason.

Issuing the session a login receives when no second factor gates it uses one
`BEGIN IMMEDIATE` transaction in the same way: it reads the Module's enabled
setting, tests whether the account holds a factor for that Module, and inserts
the session, returning without writing when both hold. It touches no watermark,
because no code was presented.

Completing a checkpoint deletes every watermark row inside the same transaction
that installs the replacement state, alongside the live session rows, so a
Restore cannot judge a newly presented code against a history belonging to the
replaced state.

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

Retained inspection exists only for a pre-operational deployment, whose
lifecycle state must be classified without mutating anything. Before it opens
SQLite, the backend checks the trusted derived WAL path through non-mutating
filesystem metadata. An existing WAL returns an uninspectable retained-state
result to the lifecycle boundary, which classifies it as the generic
`lifecycle_interrupted` / `operator_redeploy_required` action. The backend does
not open, copy, inspect, recover, checkpoint, clean up, or otherwise modify the
original database, WAL, or shared-memory artifacts in that case. This refusal is
required rather than conservative: the inspection open below is immutable, so it
would silently ignore the WAL and report stale main-file state as though it were
the retained state, and no automatic reconciliation of an interrupted Init or
Restore is safe.

A sealed deployment never reaches this path. The lifecycle boundary classifies
an initialized record from the record alone and loads it through the ordinary
read-write `SqliteDatabase::open` above, which lets SQLite recover the WAL
normally before the deployment binding and initialized state are re-verified.

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

Closing consumes the backend and its one connection. Because the backend keeps
the database in write-ahead logging mode, simply dropping the connection could
leave committed work in the WAL sidecar, and a retained WAL is exactly what
makes a pre-operational deployment uninspectable and forces an operator
redeploy. The close therefore checkpoints before it releases: it runs
`wal_checkpoint(TRUNCATE)` and requires both that the checkpoint was not blocked
and that no frames remain, then closes the connection explicitly rather than
letting it drop. SQLite removes the `application.sqlite3-wal` and
`application.sqlite3-shm` sidecars when the last connection closes cleanly, so a
cleanly stopped Server leaves only the main database file and the next start
classifies from it directly. A checkpoint that is blocked or leaves frames, or a
close the driver refuses, reports `Unavailable` instead of a clean close, which
the Server reports as an incomplete shutdown. The Server retains the blocking
close through its reporting threshold and waits for it to return, so an overdue
checkpoint is not discarded while the runtime tears down.

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

Migration tests apply the six-migration prefix to populated account and Group
tables, then prove `0007` preserves broad Unicode names and state identifiers,
assigns distinct canonical values derived from neither, installs the indexed
lookup, and rolls schema, data, and ledger changes back together after an
injected later failure. Application-state tests prove typed account and Group
lookups, foreign-key ownership, per-kind and cross-kind uniqueness, exact text
checks, immutable associations, indexed reference lookup, exact round trips
across reopen, and integrity failure for missing, extra, or wrong-kind
associations.

Close tests build a real database whose committed write is still held only in a
non-empty WAL. They prove that a clean close leaves neither the WAL nor the
shared-memory sidecar behind while the committed write survives in the main
database file, and that a checkpoint blocked by another reader reports
`Unavailable` rather than a clean stop while still releasing the connection.

Application-state tests additionally cover a full round trip of every state type
across reopen, one-time checkpoint replacement, mismatched-checkpoint and
mismatched-deployment rejection without writing state, complete rollback of a
partially written replacement, malformed persisted state, one-time obligation
acknowledgement, and an explicit assertion over the installed `weavelit_*`
tables and columns that no log-record or Log Module credential storage exists
and that `weavelit_session` is the only table naming session, token, or CSRF
data and holds exactly its seven expected columns. Further tests prove that
completing a checkpoint clears every stored session, that a rejected completion
leaves stored sessions untouched, and that stored sessions do not change the
normalized aggregate a backup is built from.

Session tests inject every instant, so no assertion depends on wall-clock timing
or on a sleep. They cover survival across reopen, the exact idle and absolute
boundaries in both directions, refusal without destruction or activity update
when the clock moves backwards, CSRF rotation on a usable session and refusal on
an expired one, scoped revocation, removal of exactly the sessions past each
boundary when a new session is issued, the bound on how many rows one issue
removes, and schema refusal of a plaintext-sized digest, of the reserved
all-zero digest, and of any direct statement that would extend or reassign a
stored lifetime.

The MVP scope does not defer quality obligations: every selected behavior has
its required security, diagnostics, safe failure, and automated test evidence
from the outset.

## Related Documents

- [Application Database Design](../application-database-design.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
- [Authentication Design](../../authentication/authentication-design.md)
- [Temporary Password Disclosure Decision](../../authentication/temporary-password-disclosure-decision.md)
