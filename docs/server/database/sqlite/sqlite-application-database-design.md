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
`0007_add_audit_references.sql`, and
`0008_add_audit_terminal_recovery.sql`, and
`0009_add_log_configuration_generations.sql`, and
`0010_migrate_totp_component_enablement.sql`, and
`0011_add_log_configuration_audit_references.sql`, and
`0012_add_account_public_identities.sql`, and
`0013_add_account_credential_state.sql`, and
`0014_add_group_public_identities.sql`. Each
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

`0011_add_log_configuration_audit_references.sql` adds the separate `STRICT`
`weavelit_log_configuration_audit_reference` table. Its owner is the existing
Log Module configuration identifier, while its value has the same canonical
random Audit Reference form and remains immutable. Reciprocal triggers reject
reuse with account or Group references regardless of insertion order. The
migration backfills every existing Log Module configuration from SQLite
`randomblob`, and table creation, indexes, triggers, complete backfill, and the
ledger row share one immediate transaction. A collision or later failure rolls
the database back to the exact `0010` prefix. Init and Restore instead supply
their already validated typed references in the application-state replacement
transaction, and state reads require exact configuration coverage and global
cross-kind uniqueness.

`0012_add_account_public_identities.sql` creates the separate `STRICT`
`weavelit_account_public_identity` table. Its account identifier is both the
primary key and a foreign key to `weavelit_account`; its public identifier is an
exact nonzero 16-byte BLOB protected by a unique index. Update and delete
triggers make every association immutable. The migration materializes up to
eight SQLite `randomblob` candidates for every existing account, rejects zero
and duplicated candidates, and selects the first remaining candidate for each
account. Table creation, index, triggers, complete backfill, and the ledger row
share one immediate transaction. Exhausting the candidate pool, an orphan, or
a later failure rolls the database back to the exact `0011` prefix so reopening
can retry the migration from unchanged legacy state.

Init and Restore completion insert their already validated typed identities in
the application-state replacement transaction. State reads require exact
account coverage and global public-identifier uniqueness. The SQLite
`AccountAdministrationStore` performs deterministic list ordering by the unique
username and exact lookup by the typed Account Public Identifier. Both queries
join `weavelit_account` to `weavelit_account_public_identity` and select only
the public identifier, username, display name, active state, and MFA-required
state. Before either query returns, the same read transaction verifies complete
reciprocal coverage, decodes every public identifier, and independently rejects
duplicates. Missing, malformed, orphaned, or duplicate identities therefore
return `IntegrityFailure` without partial output or an internal-identifier
fallback; an unknown valid public identifier returns absence. These reads do
not touch sessions or Audit terminal storage. The normal schema validation
performed during database open rejects a missing or changed identity table,
index, or immutability trigger with `IntegrityFailure` before readiness.

`0014_add_group_public_identities.sql` creates the separate `STRICT`
`weavelit_group_public_identity` table. Its Group owner is the primary key and
foreign key, its public identifier is an exact nonzero 16-byte BLOB, and a
unique index prevents reuse. Updates are forbidden. Owner deletion cascades the
identity only as part of Group deletion. The migration materializes up to eight
SQLite `randomblob` candidates for every existing Group, rejects zero and
duplicated candidates, and selects the first remaining candidate for each
Group. Table, index, trigger, complete backfill, and ledger row share one
immediate transaction. Exhausting the candidate pool, an orphan, or a later
failure rolls the database back to the exact `0013` prefix so reopening can
retry the migration from unchanged legacy state.

SQLite Group administration reads join only Group name, nullable description,
and public identity and validate complete reciprocal coverage before output.
Create, update, and delete use `BEGIN IMMEDIATE`, recheck the issuer session and
complete prepared target, and persist the selected opaque Audit terminal in the
same transaction. Delete first proves both membership and direct-grant absence,
then removes the Audit Reference and owning Group; the public identity cascades.
No nonempty count or cause leaves the backend.

### Immutable Log Module Configuration Generations

`0009_add_log_configuration_generations.sql` creates four `STRICT` tables for
deployment-local operational configuration history: immutable generation
snapshots, immutable ordered non-secret settings, immutable Log Type
memberships, and one current-generation pointer per application-owned Log
Module configuration. The compound generation key is the existing nonzero
16-byte configuration identifier plus a nonzero eight-byte big-endian version
BLOB. The BLOB representation preserves the complete nonzero `u64` range
without narrowing through SQLite's signed integer type.

The migration backfills every existing configuration as version `1`, copies
its current settings and Log Type assignments into that snapshot, and points
the configuration at version `1`. Snapshot, setting, and membership tables
reject updates and deletes through schema triggers. Their creation, complete
backfill, immutable triggers, and migration-ledger entry share the migration's
single immediate transaction; any failure rolls the schema, data, and ledger
back to the exact `0008` prefix.

Fresh Init and Restore completion seed the same version `1` rows after writing
the supplied `ApplicationState` and before marking lifecycle state initialized.
Seeding runs inside the completion operation's existing immediate transaction,
so generation failure also rolls back every application-state row and retains
the pending checkpoint.

The SQLite implementation returns its read-only store through
`ApplicationDatabase::log_configuration_generations`. Current Audit lookup
starts from the current restorable Audit assignment, requires its current
pointer and exact generation, and verifies configuration identity, version,
module, name, enabled state, ordered settings, and Audit membership against the
current application-state rows. Exact historical lookup reads only the
requested compound key. Both paths rebuild bounded contract types through
database persistence authority and return payload-free `IntegrityFailure` for
malformed or inconsistent rows; an absent exact historical key returns `None`.

The backend also returns its internal mutation store through
`ApplicationDatabase::log_configuration_mutations`. Preparation uses one read
transaction to load and validate every current configuration, setting,
assignment, immutable snapshot, and pointer. It returns an exact no-op without
writing; otherwise it allocates one next version per distinct affected
configuration and carries the complete expected and desired assignment sets.

Commit uses `BEGIN IMMEDIATE`. It first rechecks the complete expected
assignment topology and each affected current generation. A stale plan persists
only its selected opaque denied terminal. A matching plan appends immutable
snapshots, updates current enabled state, settings, and assignments, persists
only its selected applied terminal, and updates generation pointers last. Every
step and the selected terminal obligation commit together or roll back
together. This internal store adds no public route, public identifier,
destination credential storage, generation deletion, or supersession behavior.

These four tables survive ordinary restart but remain outside
`ApplicationState`. State reads and writes do not enumerate or import them, a
backup includes only current non-secret Log Module configuration and
assignments, and Restore seeds fresh version `1` rows from that normalized
input. Source-deployment generation history and pointers therefore never enter
a replacement database, and the backup format and version remain unchanged.

### Account Credential State Migration

`0013_add_account_credential_state.sql` adds an exact nonzero eight-byte
big-endian `credential_revision` BLOB, a `0` or `1`
`must_change_password` integer, and an optional nonnegative
`temporary_credential_expires_at_milliseconds` integer to
`weavelit_account`. Existing rows receive revision `1`, a false flag, and no
expiry. Column constraints and insert and update triggers require the flag and
expiry to be present or absent together. The backend rebuilds their checked
contract types on read, and `ApplicationState` additionally requires every
temporary account to have a password verifier.

The migration stores only the existing Argon2 verifier and bounded metadata;
plaintext temporary passwords, response buffers, delivery content, and
continuation bearer values have no SQLite column. It adds no route, retrieval
endpoint, response store, or password-change ticket. Account creation and
password reset use the existing schema through separately implemented
compare-and-set transactions; no later migration or backup-format change is
required.

The account credential writer uses one `BEGIN IMMEDIATE` transaction. It
rechecks exact issuer session ownership, Client Module, and lifetime; active
ordinary actor credential state and revision; current TOTP factor identity and
Module enablement; and atomically advances the replay watermark when enrolled.
A create prechecks username and generated identity collisions before inserting
the account, public identity, Audit Reference, and verifier. A reset verifies
the exact public-identifier association, compare-and-sets the target revision,
replaces or creates the verifier, and deletes every target session. Success,
duplicate or stale conflict, and final issuer denial each select exactly one
opaque terminal obligation. The selected obligation and any watermark or
business writes commit together or roll back together.

The password-change writer also uses `BEGIN IMMEDIATE` and the existing Account,
password-verifier, session, and Audit terminal tables. It joins the presented
session to the live Account to derive `PasswordChangeRequired`, then rechecks
the exact session digest, actor, Client Module, lifetime, active state,
credential revision, unexpired temporary metadata, and current verifier. On a
match it advances the revision, clears the flag and expiry, replaces the
verifier, deletes every Account session, inserts the prepared fresh session at
the successor revision, and persists the selected success terminal. A mismatch
persists only the denied terminal. A verifier update anomaly, fresh-session
collision, or terminal persistence failure rolls the complete transaction back.
This writer adds no migration or backup-format field and does not read or write
MFA replay watermarks.

The account status writer uses the same existing account, public-identity,
Audit-reference, session, and terminal-recovery tables; it adds no migration or
backup field. Its `BEGIN IMMEDIATE` transaction rechecks issuer session
ownership, actor, Client Module, lifetime, and active state before evaluating
the target. One update compare-and-sets the target account identifier, exact
public identifier association, active flag, and eight-byte credential revision.
Disablement writes inactive state and the checked successor revision, then
deletes every target session. Re-enablement writes active state with the same
revision and performs no session insert. The selected success or payload-free
denied terminal is persisted before commit, so a terminal failure rolls back
the status, revision, and session deletion together.

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

Session insertion reads the account's active flag, credential revision, and
temporary-credential expiry in the same immediate transaction that would write
the session. An absent or inactive account, a stale expected revision, or an
expiry at or before the session issue instant returns one reason-free rejection
and commits no session. Direct MFA admission, accepted TOTP steps, and confirmed
enrollment use the same check before any session, replay watermark, or factor
write, so a concurrent account-state change rolls back the whole operation.

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

## Live Audit Terminal Recovery Schema

`0008_add_audit_terminal_recovery.sql` creates private `STRICT`
`weavelit_audit_terminal_obligation` and
`weavelit_audit_terminal_supersession` tables. They are live operational
storage, not normalized application state, Log Module destination tables, or
backup content. `ApplicationState` reads and writes never reference either
table, Restore imports neither table, and the SQLite backend returns its private
store through `ApplicationDatabase::audit_terminal_recovery`.

An obligation row stores an insertion sequence, nonzero 16-byte identity,
1 to 50,176 byte opaque projection, separately stored nonzero 16-byte binding
identity, separately stored nonzero eight-byte big-endian binding version, and
an acknowledgement tombstone. The backend never interprets the projection as
JSON or extracts an Audit field from it. The big-endian version representation
preserves the complete nonzero `u64` contract range without a signed SQLite
integer narrowing.

A supersession row stores the original and replacement obligation identities,
1 to 1,024 opaque disposition bytes, and separate exact original and replacement
binding columns. Foreign keys require both retained obligations, while unique
identity constraints prevent one original or replacement from participating in
conflicting dispositions. Triggers reject obligation deletion, immutable-field
rewrites, acknowledgement reversal or repetition, and every disposition update
or deletion. Acknowledged rows remain as tombstones so an exact write or
supersession retry is idempotent and cannot resurrect completed recovery.

The private SQLite transaction adapter implements
`AuditTerminalRecoveryTransaction` only for an owning serialized mutation. It
does not expose standalone enqueue. A new write first compares any existing row
by exact identity, projection bytes, binding identity, and binding version. An
exact match succeeds without writing; any difference returns `InvalidState`.
Supersession requires the exact oldest active original, compares its complete
opaque bytes and separate binding columns, inserts the replacement, and appends
the disposition in that same caller-owned transaction. An injected failure at
either insert rolls back both. An exact completed supersession retry succeeds;
partial or byte-different retained state fails without mutation.

Active replay selects unacknowledged rows without a disposition by insertion
sequence. Late delivery selects unacknowledged disposition originals by their
original insertion sequence. Reads decode only storage shape and bounds; late
reads additionally require the disposition's separate bindings to equal the
stored original and replacement bindings. They never parse the projection or
disposition. Malformed storage shape returns `IntegrityFailure`; a bounded but
semantically malformed projection is returned opaquely for Server Audit to
reject through the runtime recovery-required path.

Acknowledgement runs in one immediate transaction. It requires exact identity
and binding proof, validates the retained row and any disposition relationship,
requires the row to be oldest in its active or late sequence, and changes only
its acknowledgement tombstone. Wrong-binding, absent, repeated, and
out-of-order acknowledgement returns `InvalidState` without mutation.

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

`0010_migrate_totp_component_enablement.sql` normalizes an initialized
deployment's former `mfa.totp` / `enabled` configuration entry into the generic
`totp` / `mfa-module.enabled` component entry. Exact legacy `true` becomes
canonical `true`; legacy false or any other legacy value becomes canonical
`false`, replacing a conflicting canonical row. When the legacy entry is absent,
an existing canonical value is preserved; when both entries are absent,
canonical `false` is inserted. The legacy row is then removed in the same
migration transaction. An uninitialized database is unchanged;
Init explicitly seeds canonical TOTP disablement when it creates application
state. Reopening repeats no data change because the migration ledger records
the exact migration once.

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

The enablement mutation uses one `BEGIN IMMEDIATE` transaction to count
distinct enrolled account identifiers, select one of the two opaque validated
Audit terminal writes, and persist that obligation. The applied branch writes
`totp` / `mfa-module.enabled` and deletes every session belonging to an enrolled
account only when disabling. The stale-preview branch writes no configuration
or session state and commits only its conflict obligation. An obligation insert
failure rolls back configuration and session changes with it.

The enablement preview uses one read transaction to load the canonical
`totp` / `mfa-module.enabled` value and count distinct enrolled account
identifiers. The current Log configuration list likewise uses one read
transaction to load every current immutable generation, validate its pointer
against current configuration, settings and Log Type membership, validate the
complete System and Audit assignment topology, and sort by unique configuration
name. Both reads use the existing schema; no migration is required.

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

Generation migration tests open a populated real database carrying the exact
eight-entry migration prefix, prove version `1` backfill and the ninth checksum,
and inject a final migration failure to prove schema, data, and ledger rollback.
State tests prove fresh Init seeding, current and exact historical reads across
restart including the full `u64` version range, immutable-row enforcement,
completion rollback, and fail-closed malformed version, pointer, configuration,
enabled-state, settings, and Audit-membership handling. A source-to-replacement
test proves normalized Restore state excludes historical generations and that
the replacement creates only its local version `1` snapshot.

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
