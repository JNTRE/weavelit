# Application Database Design

This document defines the shared implementation design for the Server's
internal **[Application Database](../../glossary.md#applications-and-interfaces)**
backend contract. Backend-specific storage behavior belongs in the applicable
child directory.

## Crate Boundary

`weavelit-server-database` defines the backend-neutral Application Database
contract: Server domain types, the persistence operations available to the
Server, and storage-neutral typed errors. It contains no backend selection,
driver, connection, query, transaction, migration, or backup implementation.

`weavelit-server-database-authority` is the dependency-free, unpublished
capability crate that gates persisted Audit Reference decoding, immutable Log
Module configuration-generation materialization, and opaque Audit terminal
recovery decoding. Its privately represented `ServerDatabaseAuthority` is not
reexported by the database contract or lifecycle crate. Lifecycle is the sole
production authority that constructs a selected database binding. The database
contract declares the capability only as a persistence-capability factory's
input, and direct persistence test support declares its own reviewable dev
dependency rather than obtaining authority through the public backend-neutral
contract.

Each supported backend is a dedicated compiled-in implementation crate. The
MVP SQLite implementation is `weavelit-server-database-sqlite`. It implements
the shared contract and owns all SQLite-specific behavior. An Application
Database backend is not a runtime **[Module](../../glossary.md#applications-and-interfaces)**.

The **[Weavelit Server](../../glossary.md#applications-and-interfaces)** owns
backend composition. The `weavelit-server-lifecycle` crate presents the
available compiled-in backends during the shared pre-operational workflow,
validates and persists the selected backend's minimum connection configuration
in a protected Server-local locator, constructs that backend, and calls it
through the shared contract. Each backend validates its own connection and
storage settings. Backend declarations expose only typed connection values that
a client may supply; they never expose local filesystem paths or file
references. The Server derives every local path and owns creation, placement,
protection, replacement, and removal of backend files. A future backend
independently selects its own connection and concurrency model behind that same
contract.

## Initial Contract

The initial contract expresses Server application intent rather than storage
mechanics. It supports only these capabilities:

1. Inspect whether the Application Database is uninitialized,
   `InitializationPending`, or initialized, including the deployment identifier
   and workflow discriminator bound to pending or initialized state.
2. Atomically create a non-operational Init or Restore checkpoint containing
   the deployment identifier, workflow discriminator, and only the non-secret
   metadata defined by that workflow's contract.
3. Atomically replace an eligible Init checkpoint with complete new application
   state bound to that deployment identifier exactly once.
4. Atomically replace an eligible Restore checkpoint with complete validated
   restored application state bound to that deployment identifier exactly once.
   The backend receives normalized state and never parses, decrypts, stages, or
   validates a backup artifact or private recovery key.
5. Load the initialized application-owned state and deployment identifier
   required by Server startup.
6. Acknowledge the persisted completion obligation of a finished Init or
   Restore workflow exactly once.
7. Close the open database, releasing it and leaving no work for recovery.

Closing consumes the backend, so a use after close is not expressible rather
than detected. The Server holds one open handle for the deployment's lifetime
and closes it through a single process-wide owner during shutdown; that owner
takes the handle out of every clone at once, so the backend's close runs exactly
once however many times shutdown is requested, and every later operation is
refused as unavailable. A backend that cannot leave its storage free of pending
recovery work reports an unclean close rather than a clean one. The Server
retains and awaits every close it starts; its five-second reporting threshold
marks the shutdown incomplete but never permits the close to be abandoned.

The Rust contract is synchronous and object-safe. `ApplicationDatabase`
requires a movable backend and takes exclusive mutable access for every call;
it does not require a backend to be safe for concurrent shared access. The
lifecycle boundary serializes workflow mutation and decides where blocking
storage calls execute when it composes the backend. This keeps runtime and
executor dependencies out of the persistence contract while allowing a future
backend to implement the same operations with its own connection model.
Implementing the trait grants no decoder-issuance method. Persisted reads that
can decode an Audit Reference instead require an explicit borrowed
`AuditReferencePersistence`, while backend factories continue to create only a
raw backend-neutral trait object.

`DeploymentIdentifier` is an opaque 16-byte value. The lifecycle boundary
generates it cryptographically, and the contract rejects the reserved all-zero
representation. The contract exposes only its binary representation and
redacts it from diagnostic formatting. A migrated database with no checkpoint
or application state is unbound. Creating the first checkpoint binds the
database to its deployment identifier. Pending and initialized state always
carry the persisted binding.

`DatabaseInspection` represents uninitialized, pending, and initialized state.
A pending result contains a `WorkflowCheckpoint` whose `WorkflowKind`
discriminates Init from Restore. `CheckpointMetadata` is an immutable opaque
byte sequence limited to 4 KiB. The owning workflow defines and validates its
encoding and meaning, ensures that it contains only non-secret values, and
decides whether an empty value is valid. The Application Database stores,
returns, and compares those bytes exactly but never parses or interprets them.
Diagnostic formatting reports only the metadata length.

Inspection is read-only. An absent lifecycle-state record returns
`Uninitialized`. A valid persisted deployment identifier is compared with the
trusted expected identifier before workflow state is returned; a different
identifier returns `DeploymentMismatch`. Invalid persisted identifiers,
cardinality, state values, workflow discriminators, metadata bounds, or
contradictory field combinations return `IntegrityFailure`.

Inspection receives the trusted expected deployment identifier and rejects a
different persisted binding. Checkpoint creation receives the complete desired
checkpoint. Checkpoint creation is one-shot: it succeeds only while no
lifecycle-state row exists. Any pending checkpoint rejects another creation
with `InvalidState`, including an identical request; it is retained for
fail-closed lifecycle-interruption classification rather than a retry, reset,
or discard operation. An initialized row returns `AlreadyInitialized`. After a
valid persisted identifier is decoded, a different deployment returns
`DeploymentMismatch`; malformed durable state returns `IntegrityFailure`.
Every mutation validates current state under the same serialized transaction
that performs its write. The contract exposes no transaction, query, migration,
path, connection, backend-selection, reconciliation, or checkpoint-discard
mechanism.

Each final write persists the complete supplied state and marks the database
initialized as one operation. A pending checkpoint is not application state and
permits only the workflow identified by its discriminator. An Init checkpoint
prevents a new key pair from being generated while the requesting client proves
possession of the delivered private key. A Restore checkpoint prevents Init or
a second Restore attempt from replacing its in-progress workflow. If a final
write is interrupted, the checkpoint is retained for fail-closed
lifecycle-interruption classification; the database does not offer resume or
reset. Every later Init or Restore attempt returns the stable
`AlreadyInitialized` error.

The backend compares the expected deployment identifier on every checkpoint,
finalization, load, restore, and acknowledgement operation. It rejects a
mismatch before changing state. The identifier is an integrity binding between
this database and the Server-local deployment record and locator; it is not an
authentication credential or secret.

## Application State Model

The contract carries deployment-bound application state as a bounded typed
aggregate of normalized entities, not as an opaque serialized snapshot. Each
recoverable state type is a distinct type with its own validated fields, so a
backend persists it in normalized relational form and the Server can reason
about, migrate, and integrity-check individual entities. `ApplicationState`
covers component configuration, protected component secrets, local accounts,
password verifiers, Groups, Group memberships, Group grants, protected MFA
factor data, Service Connection credentials, the persisted recovery public key,
non-secret Log Module configuration and settings, typed Audit Reference
projections for every account, Group, and Log Module configuration, System Log
and Audit Log assignments, and the workflow completion obligation.

The aggregate has no session, Log Module destination data, immutable Log Module
configuration-generation history, normal-operation Audit terminal recovery
obligation, or Log Module credential member. Active sessions, System Logs and
Audit Logs, other Log Module destination data, generation history, live terminal
recovery obligations, and Log Module authentication or connection credentials
therefore cannot enter persisted application state through this contract.

Live sessions are still stored in the Application Database, through the separate
`SessionStore` contract described in [Live Session Storage](#live-session-storage).
They are live operational data rather than restorable state: they survive an
ordinary restart, they never appear in normalized state or in a backup, and a
Restore clears them. The same separation applies to normal-operation Audit
terminal recovery obligations described below. A backend has no schema in
which to act as a Log Module destination or store Log Module credentials.

### Immutable Log Module Configuration Generations

The backend-neutral contract defines an internal immutable generation key as
the existing Log Module configuration `StateIdentifier` plus a nonzero
`LogConfigurationVersion`. Version `1` is `INITIAL`. The key has no text codec,
Serde representation, random generation identifier, or public API identifier;
its diagnostic representation exposes neither the state identifier nor the
version.

Each `LogConfigurationGeneration` is a non-secret immutable snapshot containing
that exact key, the committed Log Module and configuration name, enabled state,
canonically ordered non-secret settings, and canonically ordered Log Type
membership. Settings and memberships must be unique. The snapshot carries no
credential, protected setting, destination handle, Log-owned binding, or Log
authority. `LogConfigurationGenerationPersistence`, issued only from
`ServerDatabaseAuthority`, is the only public construction path for keys and
persisted snapshots; ordinary contract consumers cannot forge either value.

`LogConfigurationGenerationStore` is read-only. It can load the generation
currently assigned to Audit Logs or one exact historical generation by key. It
does not expose enumeration, mutation, version allocation, supersession, route,
or public-identifier behavior. `ApplicationDatabase::log_configuration_generations`
defaults to `None` as an explicit staged backend decision: existing backends
remain source-compatible, and a caller requiring generation reconstruction
must treat absence as unavailable rather than fall back to mutable state.

`LogConfigurationMutationStore` is the separate optional internal mutation
surface. A backend atomically prepares a canonical request from its current
configuration rows, settings, complete assignments, immutable generations, and
current pointers. An exact no-op returns `Unchanged` before an Audit Attempt or
write. A prepared change carries each affected configuration's exact expected
generation and one next-version candidate, the complete expected and desired
assignment topologies, and every resultant current generation needed for
Server-owned validation and destination preflight. Moving an assignment
advances both its old and new configuration exactly once, even when the primary
configuration also changes settings or enabled state.

Commit rechecks the exact affected generations and complete expected assignment
topology in one serialized backend transaction. A mismatch commits only the
prevalidated stale terminal obligation and no configuration, generation,
assignment, or pointer change. A match atomically appends all candidate
generations, updates current application state and assignments, persists only
the applied terminal obligation, and advances every affected current pointer
last. The contract does not expose public routes, wire identifiers, destination
credentials, generation deletion, or terminal supersession.

The MVP SQLite backend implements this store through deployment-local
operational tables. Migration and checkpoint completion seed version `1` from
the current restorable Log Module configuration, settings, and assignments.
The store preserves immutable history across ordinary restart, validates the
current Audit pointer and snapshot against current application state, and
fails closed on malformed or inconsistent rows. Exact historical reads do not
fall back to mutable state or another generation. This persistence does not
change the Audit terminal recovery contract or make the Application Database
depend on Log or logging-authority types.

Generation history and its current pointers are not members of
`ApplicationState`, backup content, or normalized Restore input. A replacement
deployment receives only the restored current non-secret configuration and
assignments, then creates its own version `1` snapshots in the same transaction
that completes Restore. Source-deployment history is neither imported nor
merged, and this operational persistence does not change the backup format or
version.

### Live Audit Terminal Recovery

The **[Application Database](../../glossary.md#applications-and-interfaces)**
recovery contract owns only bounded opaque storage values. It has no dependency
on the Log contract, logging authority, a Log Module, or any Log-owned recovery,
binding, disposition, or acknowledgement type. A backend implementation must
not parse or materialize Audit fields, mint logging authority, become an Audit
Log destination, or store queryable Audit records.

Each stored obligation has a nonzero 16-byte opaque identity, a projection of
1 to 50,176 uninterpreted bytes, and a separately stored binding made of a nonzero
16-byte opaque identity and nonzero `u64` version. An append-only supersession
has a disposition of 1 to 1,024 uninterpreted bytes plus separately stored exact
original and replacement bindings. The projection and disposition types expose
no field or semantic accessors. Diagnostic representations and contract errors
are payload-free.

`AuditTerminalRecoveryPersistence` is issued only from
`ServerDatabaseAuthority`. It gates persisted-row decoding and the private-field
validated write, supersession, and acknowledgement-proof wrappers. Server Audit
uses that capability to convert its already validated terminal projection and
binding into a database write, to import opaque database rows for semantic
validation, to convert its validated fixed disposition into a supersession
write, and to convert exact destination acknowledgement into database proof.
The database contract cannot construct any of those facts from Log authority
and never accepts a Log-owned value.

`AuditTerminalRecoveryTransaction` is available only inside a consequential
application-state mutation transaction. That transaction persists the
authoritative mutation and its immutable validated-write wrapper together, or
commits neither. The backend-neutral contract deliberately exposes no
standalone enqueue operation that could add an obligation after a mutation had
already become durable. Repeating the exact identity, projection bytes, and
binding is idempotent; reusing an identity with any byte or binding difference
returns `InvalidState` without mutation.

`AuditTerminalRecoveryStore` exposes separate bounded oldest-first sequences for
active pending obligations and superseded originals retained for late delivery.
It acknowledges only the exact oldest eligible identity in the applicable
sequence; an absent, repeated, or out-of-order acknowledgement returns
`InvalidState`. Server Audit first imports and semantically validates the opaque
projection, requires its embedded identity and binding to equal the separately
stored columns, resolves the trusted binding-and-destination pair, and converts
the destination's exact acknowledgement into the private database proof. A
malformed projection, changed binding, delivery failure, import failure, or
acknowledgement failure leaves the obligation pending. A runtime encountering
an opaque bounded row that Server Audit cannot import enters the owning
`RecoveryRequired` failure path rather than asking the backend to interpret or
repair it.

`AuditTerminalSupersession` retains the exact Server Audit-validated original
obligation bytes and separately stored binding together with the opaque
Server Audit-validated disposition, exact original and replacement bindings,
and distinct replacement validated write. The backend compares only those
opaque bytes and separate columns. It accepts the write only when the exact
stored original is the oldest active obligation and no disposition exists;
matching identity alone is insufficient. It atomically appends the disposition
and replacement obligation in the same transaction where the owning
configuration workflow applies the replacement assignment. An exact repeat is
idempotent. Any byte or binding mismatch, partial prior state, duplicate with
different content, or non-oldest request returns `InvalidState` without
mutation. The original remains immutable and moves only to the late-delivery
sequence.

These obligations are defined as live operational data that a conforming store
must preserve across ordinary restart. They are not members of
`ApplicationState` and cannot enter Application Database backups or normalized
Restore input. Restore therefore imports no source-deployment obligation; a
replacement deployment begins with none inherited from the backup. This
contract defines no in-place Restore clearing operation. Obligations are also
absent from every Log Module destination backup unless and until that
destination has acknowledged the replayed record.

### Account Credential Writers

Every account record carries a nonzero monotonically increasing credential
revision, a `must_change_password` flag, and an optional nonnegative absolute
temporary-credential expiry instant. Revision `1` is the initial and legacy
default. The flag is true exactly when the expiry is present, and temporary
metadata is valid only for an account that has a password verifier. An ordinary
credential has a false flag and no expiry. The aggregate carries no temporary
password, response content, delivery content, or continuation bearer.

`AccountCredentialWriterStore` implements transport-independent account create
and password-reset mutations. A reset-target preparation read resolves one
exact Account Public Identifier, internal account, credential revision, and
typed Audit Reference in one backend snapshot. Each final mutation receives a
non-clonable exact-session recheck, the prepared verifier and 24-hour metadata,
and prevalidated opaque Audit terminal alternatives.

The final transaction rechecks exact session ownership, Client Module, and
liveness; active ordinary actor credential state and revision; current factor
identity and Module enablement; and the verified TOTP replay step when
enrolled. Create writes revision `1`, active optional-MFA state, independently
generated internal, public, and Audit identities, and no Group, grant, factor,
watermark, or session. Reset compare-and-sets the target revision, replaces or
creates its verifier, preserves its other Account state, and revokes every
target session. Each outcome commits exactly one selected opaque Audit terminal
with the business decision or rolls back both.

The contract adds no route, response encoding, or request-idempotency store.
The Application Database remains separate from the Audit Log destination. Refer to the
[Authentication Design](../authentication/authentication-design.md#account-credential-issuance-writers)
for expiry, disclosure, session, and reauthentication policy.

### Password Change Writer

`PasswordChangeWriterStore` owns the atomic replacement of one temporary
credential through its exact restricted session. Its prepared mutation carries
the account, session digest, issuing Client Module, expected credential
revision, exact current verifier, replacement verifier, decision instant, and
one fresh session and CSRF digest pair bound to the checked successor revision.
It carries no plaintext password, temporary credential, TOTP code, factor,
watermark, public identifier, response envelope, or cookie.

The final transaction rechecks the exact session's ownership, Client Module,
and lifetime together with the active Account, expected revision,
`must_change_password`, unexpired temporary metadata, and exact current
verifier. A match advances the revision, replaces the verifier, clears the
temporary flag and expiry, revokes every account session including the proof
session, inserts exactly the prepared fresh session, and stores only the
selected success Audit terminal. A missing, revoked, stale, expired, disabled,
ordinary, or verifier-mismatched state performs no credential or session
mutation and stores only the selected denied terminal. A session-digest or
Audit-terminal collision fails the transaction and commits nothing.

This writer neither verifies a password nor advances an MFA replay watermark.
The Server authentication boundary owns the non-forgeable restricted-session
proof, same-password refusal, approved verifier preparation, fresh bearer
ownership, and postcommit result. The writer adds no schema, migration, backup
field, route, or public identifier contract.

### Account Status Writers

`AccountStatusWriterStore` implements transport-independent disable and
re-enable mutations for a local **[Human User](../../glossary.md#identities-and-access)**.
A preparation read resolves one exact Account Public Identifier, internal
account identifier, typed Audit Reference, active state, and credential
revision in one backend snapshot. An exact desired-state match is an unchanged
result before an Audit Attempt or writer call. Preparing disablement computes
the checked next credential revision; exhaustion is a stable pre-commit
rejection. Preparing re-enablement retains the current revision.

The final transaction first rechecks the issuing **[Administrator](../../glossary.md#identities-and-access)**'s
exact session digest, actor, Client Module, lifetime, and active state. It then
compare-and-sets the target's public identity, active state, and credential
revision. Disablement writes inactive state, advances the revision, and deletes
every target session. Re-enablement writes active state only and neither changes
the revision nor creates or restores a session. Self-disablement is valid: the
issuer recheck completes before target-session deletion removes that same
session.

Success selects one prevalidated success Audit terminal. A stale target or
final issuer denial selects one payload-free denied terminal and performs no
business mutation. The selected opaque terminal and all business effects commit
together or roll back together. The writer does not change the verifier,
temporary-credential metadata, MFA requirement, factor enrollment, replay
watermark, Group membership, grant, public identifier, or Audit Reference.
Status and revision remain ordinary restorable account state, while sessions
remain live operational data that Restore clears.

## Live Session Storage

`SessionStore` is a separate backend-neutral contract from `ApplicationState`,
so session data cannot reach a backup or a normalized state document by
construction. A stored session holds the session token digest, the per-session
CSRF digest, the owning account, the issuing
**[Client Module](../../glossary.md#applications-and-interfaces)**, the issue
instant, the last-seen instant, and an absolute expiry instant. It caches no
Group, grant, or other authorization data; authorization is evaluated live.

A digest is a distinct 32-byte type with one constructor that accepts a
`[u8; 32]` and rejects the reserved all-zero value. There is no constructor from
a string or from any variable-length input and no conversion from one, so no
plaintext token or CSRF value can inhabit the type or be persisted through it.
Neither digest type implements `Display`, and `Debug` renders a fixed redacted
string. A digest is compared through a constant-time equality method; the types
implement no ordinary equality.

The absolute expiry is derived once from the issue instant using the approved
profile's 12-hour maximum. It is not a caller-supplied field and is never
extended. A session is expired when the clock reaches `last_seen_at` plus the
30-minute idle timeout or reaches the absolute expiry; the instant one unit
before each boundary is still valid. If the clock has moved backwards so that
the present instant precedes either recorded instant, the session is refused
before any lifetime arithmetic is performed and its recorded activity is not
advanced, so a rolled-back clock fails closed rather than granting a longer
lifetime. A session refused for a backwards clock is not destroyed, because the
clock rather than the session is what is wrong.

The contract provides atomic `create`, `validate_and_touch`, `rotate_csrf`,
`revoke`, and `revoke_for_account` operations. A new session carries the exact
credential revision its caller verified. Before `create` writes, the store
checks in the same transaction that the account still exists, is active, still
has that revision, and has no temporary-credential expiry at or before the
session issue instant. Every ineligible state returns the same reason-free
rejection and writes no session. The direct, second-factor, and enrollment
session operations apply the same check before any session, replay watermark,
or factor write.
`validate_and_touch` and `rotate_csrf` remove a session they find expired.
`validate_and_touch` takes the presented CSRF digest and compares it inside the
same transaction that resolves the session, and it advances the recorded
activity only when the digests match, so a request failing that comparison
cannot extend the idle timeout. A mismatch is reported as the same rejection an
unknown session token produces.
Completing a state replacement clears every stored session inside the same
atomic replacement.

Issuing a session is also what removes expired ones. Every insertion first
removes sessions already expired at the instant the new session is issued,
inside the transaction that inserts it, so no separate sweep, timer, or
unreferenced maintenance operation is needed and no expired row can accumulate
while sessions are being issued. The removal is bounded: one insertion removes
at most a fixed batch of expired rows, so a single login can never be made to
delete an unbounded number of rows and a large accumulation is drained across
subsequent insertions instead. Rows accumulate only by issuing sessions, so a
bounded batch per insertion keeps pace with the only source of growth. Removal
failure does not fail the login: the insertion still commits, because the
session's correctness does not depend on the removal, and rows left behind are
already unusable and are offered to the next insertion. A failure severe enough
to also prevent the insertion still fails the login through the insertion
itself, so no token is ever issued for a session that was not stored.

Every text field is bounded, non-empty, and free of control characters. A state
identifier is an opaque 16-byte value that rejects the reserved all-zero
representation. A password verifier is a bounded ASCII PHC string, and the
recovery public key is the canonical lowercase `age1` recipient encoding
defined by the [Server Restore Design](../lifecycle/restore/restore-design.md).

Every account carries exactly one Account Public Identifier in normalized
application state. It is an independently generated, nonzero 16-byte value
that remains stable when application state is persisted or restored. It is
distinct from the account's `StateIdentifier`, username, and Audit Reference
Identifier and has no conversion from those values, public text codec, or
ordinary raw-byte accessor. Its diagnostic representation and errors expose no
identifier value.

The Application Database contract generates new Account Public Identifiers
from operating-system randomness through the same exact-pinned `getrandom`
dependency and bounded eight-attempt all-zero rejection used for Audit
Reference Identifier entropy. Persisted encoding and decoding require a
separate `AccountPublicIdentifierPersistence` capability issued from
`ServerDatabaseAuthority`. The checked aggregate requires exact account
coverage and global Account Public Identifier uniqueness. The database contract
exposes an optional `AccountAdministrationStore` for deterministic list reads
and one exact typed public-identifier lookup. Both return only an
`AccountAdministrationProjection` containing the Account Public Identifier,
username, optional display name, active state, and MFA-required state. The
projection fields are private and have no extension field. It carries no state
identifier, Audit Reference Identifier, password verifier, MFA factor, session,
temporary credential value, or temporary credential metadata.

Every list or exact lookup validates complete account-to-public-identifier
coverage and every stored identifier before returning output. Invalid coverage,
malformed values, or duplicate identifiers fail with a payload-free database
integrity error rather than omitting an account or substituting an internal
identifier. An unknown valid typed public identifier returns absence. The
contract defines no mutation, string parsing or encoding, public route,
response, cursor, or pagination behavior.

Every account, Group, and Log Module configuration carries exactly one
**[Audit Reference Identifier](../../glossary.md#applications-and-interfaces)**
in normalized application state. This identifier is an independent random,
nonzero 128-bit value rendered only as `ar-` followed by 32 lowercase
hexadecimal characters. Its representation is private, with no conversion from
a `StateIdentifier`, name, user string, or caller-provided bytes and no raw-byte
accessor. Diagnostic formatting and validation errors are payload-free. The
identifier is internal pseudonymous data: it is linkable and not secret, but it
is not a public API identifier.

The Application Database contract generates each new value from operating-
system randomness through exact-pinned `getrandom = "=0.4.3"`, with default
features disabled. An unavailable random source stops construction without a
device-path read, deterministic derivation, or lower-quality fallback. This
remains backend-neutral domain construction: the contract chooses only the
identifier's entropy and representation, while each backend still owns its
storage, transaction, locator, and path policy.

The reserved all-zero output is retried through the same operating-system
source at most eight times. Eight consecutive reserved values stop generation
as randomness unavailable rather than looping without a bound or substituting
a fallback.

Canonical persisted-value decoding stays private to the identifier type. An
opaque private-field `AuditReferencePersistence` capability has one issuer,
`AuditReferencePersistence::from_server_authority`, which requires a borrowed
`ServerDatabaseAuthority`. `ApplicationDatabase` has no required, default, or
hidden issuance method. After successful selection or reopening, lifecycle
uses a crate-private constructor to build `SelectedDatabase` with private fields
containing the raw backend and that decoder. SQLite's initialized-state and
typed Audit Reference reads require a borrowed decoder; lifecycle supplies the
selected value and carries it to Restore. Ordinary callers receive no selected
binding constructor, arbitrary-string constructor, `From`, `TryFrom`,
`FromStr`, or raw-byte accessor. The capability exposes only canonical nonzero
decoding and keeps its `Debug` representation redacted.

The checked aggregate requires exact account, Group, and Log Module
configuration coverage and rejects an Audit Reference Identifier reused by any
of those entity kinds. The database contract exposes typed
`AccountAuditReference`, `GroupAuditReference`, and
`LogConfigurationAuditReference` projections, looked up by the entity's
`StateIdentifier`, so later Audit construction never accepts an arbitrary
reference string. Service Connections and Automation Identities use the same
identifier type when their owning state and audit integration are implemented;
this contract does not create placeholder entities or references for absent
state.

Reversibly encrypted values are modeled as opaque protected byte payloads. The
Application Database stores and returns those bytes exactly. It never accepts
raw key material, derives keys, encrypts, decrypts, interprets, or discloses
them. The Server-local at-rest key material stays inside lifecycle ownership.

`ApplicationState` is constructed through a checked constructor that orders
every collection deterministically, rejects duplicate identifiers and duplicate
unique keys, rejects references to absent accounts, Groups, or Log Module
configurations, and requires exactly one enabled Log Module assignment for each
of the System Log and the Audit Log. Rejections use the same redacted
contract-input errors as other invalid inputs and never echo the rejected
value. A backend that decodes persisted state through the same constructor
therefore detects malformed durable state and returns `IntegrityFailure`.

The completion obligation carries the bounded, non-secret System Log fields for
the workflow-result record required by the
[Technical Specification](../../spec.md#logging-and-accountability): its record
identifier, workflow discriminator, classification, correlation identifier, UTC
Unix millisecond event time, and detail. Checkpoint replacement persists it
unacknowledged in the same transaction as the state it completes, so an
interrupted workflow cannot lose its logging obligation. Acknowledgement is a
separate one-time operation that requires initialized state, the expected
deployment identifier, and the exact persisted record identifier; a second
attempt or an unknown record identifier returns `InvalidState`. Loading
initialized state reports whether the obligation is still outstanding.

Checkpoint replacement is workflow-neutral and serves both the Init and the
Restore final write. It receives the expected deployment identifier through the
supplied checkpoint, requires the persisted pending checkpoint to equal that
checkpoint exactly, and requires the state's completion obligation to name the
same workflow. A different deployment returns `DeploymentMismatch`, a different
or absent checkpoint returns `InvalidState`, and initialized state returns
`AlreadyInitialized`.

The contract initially exposes these storage-neutral error categories:

```text
AlreadyInitialized
NotInitialized
InvalidState
DeploymentMismatch
ConfigurationInvalid
Unavailable
IntegrityFailure
```

`DeploymentMismatch` means the database is bound to another deployment
identifier and must never be initialized, loaded, or restored by this Server.
`ConfigurationInvalid` means that the selected backend configuration or
Server-local locator must be corrected. `Unavailable` means an otherwise valid
backend cannot currently be opened, queried, locked, or used. `IntegrityFailure`
prevents normal operation when persisted data, schema, migration history, or
locator integrity is damaged or incompatible. Backend-specific error details
remain private and are mapped to these safe categories before reaching the
Server. `DatabaseError` variants carry no dynamic payload and use stable,
storage-neutral display text. Invalid contract inputs use the same redaction
rule and never include the rejected identifier or metadata.

## MFA Replay Watermark Storage

`MfaStore` is a third backend-neutral contract, separate from both
`ApplicationState` and `SessionStore`. It records, for each enrolled MFA
factor, the highest time step the Server has ever accepted from that factor, so
a code presented a second time inside the acceptance window described in the
[Authentication Design](../authentication/authentication-design.md#replay-watermark)
is refused.

A time step is a distinct type whose checked constructor rejects a value a
backend could not store, and which exposes its domain value and its stored
representation separately, so the conversion at the storage boundary is total.

The contract exposes reading a factor's current watermark and one combined
accept operation. The accept operation names the Module's configuration
component, the factor, the presented step, and the session to issue. It reads
the component's enabled setting, performs the watermark comparison and write,
and writes the session, all in one transaction, and reports whether the step was
accepted, refused as a replay, or refused because the Module was disabled. It
does not return the watermark for a caller to compare itself. The decision
belongs to the store because a caller that read, decided, and then wrote would
leave a window in which a concurrent presentation of the same code could be
accepted twice, or in which a Module disabled while the code was in flight could
still have a session issued behind the disablement's own session revocation.
Nothing is written when the step is refused for either reason.

The same contract owns issuing the session a login receives when no second
factor gates it, enrolling a factor, changing a Module's enabled state, and
counting enrolled accounts, because each of those is a decision the caller must
not make from separately loaded state.

Issuing that session names the Module's configuration component, the account,
and the session. The store reads the enabled setting, reads whether the account
holds a factor for that Module, and writes the session, all in one transaction,
and writes nothing when the Module is enabled and the account holds a factor.
A caller that decided both from state it loaded earlier would write a session
for an enrolled account behind a Module enabled while the login was in flight,
and no enablement change can revoke it: enabling revokes nothing, and disabling
reaches only the sessions that already exist.

Enrolling names the Module's configuration component and the session to issue as
well as the factor: the store reads that component's enabled setting, refuses
the enrollment when the Module is not enabled, and otherwise writes the factor,
its confirming watermark, and the session, all in one transaction. It reports
whether the factor was enrolled, was already present, or was refused because the
Module was disabled. The store also exposes a target-scoped count of distinct
enrolled Human Users for the Administrator preview.

Changing enabled state accepts the expected enrolled-Human-User count and two
opaque, Server Audit-validated terminal writes: applied and count-changed. One
transaction recounts enrolled Human Users, selects exactly one terminal, and
persists that obligation. A matching count writes the canonical component
enablement and, on disablement, revokes every session belonging to an enrolled
Human User before committing the applied terminal. A changed count commits only
the count-changed terminal and returns the current affected-Human-User count.
Any state, session, or obligation failure rolls the whole transaction back.
The backend neither parses the terminal nor accepts a standalone enqueue.

The canonical TOTP entry is component `totp`, key `mfa-module.enabled`. Init
seeds it to a disabled value. Restore normalization removes the former
`mfa.totp` / `enabled` entry and emits exactly one canonical entry, preserving
an exact canonical value when no legacy entry is present and otherwise deriving
the canonical value from the legacy entry. No second mutable authority remains.

The same store accepts a current-session administration step-up for one exact
TOTP factor. It atomically rechecks the live session, actor activity, factor
ownership, and canonical TOTP enablement, then advances only the factor's replay
watermark when the presented time step is new. It neither creates nor rotates a
session. The Server converts the accepted store result into a five-minute,
exact-session Administration proof; no rejected result creates a proof.

A watermark is live operational data in the same sense as a session: it belongs
to the running deployment rather than to the restorable aggregate. It is not a
member of `ApplicationState`, it never reaches a backup, and completing a state
replacement clears every watermark inside the same atomic replacement that
clears every session.

## Existing-Group Mutation Storage

The Application Database exposes a prepared-target and commit boundary for
membership and direct-grant changes on existing Groups. Preparation resolves
the exact Group, Account Public Identifier or canonical grant, persisted Audit
References, and association presence from one consistent snapshot; an absent
target remains absent and a desired no-op is reported before an Audit Attempt.

Commit accepts the exact issuer session recheck, prepared target, desired
presence, and opaque prevalidated terminal alternatives. One atomic transaction
rechecks issuer liveness and activity, Client Module, target associations, and
expected association state. For a removal it computes active effective Server
Administration Permission after the proposed change. It selects exactly one of
the success, generic-denied, or last-administrator-denied terminal obligations,
and commits that selected obligation with the row mutation or neither. The
backend never parses or invents an Audit terminal. A committed membership or
grant change becomes visible through the existing live authorization projection
on the next request; it does not change a session or populate an authorization
cache.

## Deployment Record, Locator, And Operational State

The Server-local deployment record contains a unique deployment identifier and
the lifecycle state `Uninitialized`, `InitializationPending`, or `Initialized`.
The separate locator repeats that identifier, identifies the compiled-in
backend, and contains only the typed non-secret connection settings and
Server-encrypted secret connection values needed to reopen the Application
Database. Both remain outside the Application Database, and each file write is
atomic. Locator replacement first publishes an immutable generation, then
atomically replaces the deployment record's generation pointer as the cross-file
commit point defined in the [Server Lifecycle Design](../lifecycle/lifecycle-design.md).
The locator never contains plaintext secrets or a caller-supplied path or file
reference. Application-owned operational state and the matching deployment
identifier are persisted through the database contract.

The shared lifecycle contract selects the Application Database through a
**[Client Module](../../glossary.md#applications-and-interfaces)** that declares
an **[Init](../../glossary.md#states-and-requests)** or
**[Restore](../../glossary.md#states-and-requests)** capability while the Server
is in restricted uninitialized mode. It validates database eligibility before
either workflow accepts later secrets or backup content, and the database's
final atomic write returns `AlreadyInitialized` on every later attempt.
Post-initialization administration cannot rerun either workflow or change the
selected backend. An unsafe or invalid locator, unavailable or
integrity-failing configured database, or deployment identifier mismatch fails
closed before state is read or changed and without exposing Init or Restore as
a fallback. Cross-store ordering, sealing, and retained-state interruption
classification are defined in the [Server Lifecycle Design](../lifecycle/lifecycle-design.md).

## Log Module Separation

Application Database persistence and **[Log Module](../../glossary.md#applications-and-interfaces)**
destinations remain structurally and operationally separate, even when both
use the same technology. They do not share Weavelit-owned persistence logic or
implementation crates, database files, schemas, migration ledgers, connections,
health checks, configuration, resources, lifecycle, backup or recovery behavior,
or retention policy. They may use the same workspace-pinned third-party
dependency, such as `rusqlite`, without sharing persistence behavior.

The Server rejects configuration where an Application Database file and a Log
Module database file resolve to the same file. Log Modules receive only
pre-redacted records from the Server; they never read or modify Application
Database state. The Application Database never acts as a Log Module destination,
fallback, or queue.

Application Database selection and configuration occur through a Client
Module's
**[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)**
backed by the shared lifecycle contract. Init then selects and configures
initial Log Modules; Restore imports their application-owned configuration from
the validated backup. Selecting the same underlying technology for both does
not reuse an Application Database backend or its resources.

## Backup And Restore

During **[Init](../../glossary.md#states-and-requests)**, the Server creates a
backup recovery key pair. The Server persists only the public key. The
person completing Init receives the private key once over HTTPS, proves
possession to finalize Init, and stores it outside Weavelit. This recovery key
pair is not used to protect the Server's normal database fields and is separate
from the Server-local at-rest key material used for reversibly encrypted data.

An **[Administrator](../../glossary.md#identities-and-access)** with the
**[Server Administration Permission](../../glossary.md#identities-and-access)**
can create and download an encrypted, versioned Application Database backup
through server-administration functions. For each backup, the Server creates a
fresh data-encryption key, encrypts the recovery contents with it, and protects
that data-encryption key with the persisted recovery public key. The Server
does not require, receive, store, or redisplay the private recovery key during
backup creation.

A backup includes the application configuration and state needed to restore
operational status, including local accounts, password verifiers, Groups and
their grants, enabled-module state, protected MFA factor data, Service
Connection credentials, and other application configuration. It excludes active
sessions, which are invalidated on restore, live normal-operation Audit terminal
recovery obligations, immutable Log Module configuration-generation history,
and the live lifecycle reconciliation digest. None of these operational stores
is application state: each completed Init or Restore
atomically writes its own reconciliation digest outside `ApplicationState`, and
normalized Restore input carries no source-deployment terminal obligation. A
replacement deployment therefore starts with no obligation inherited from the
backup and creates fresh version `1` Log Module configuration snapshots from
the restored current configuration. For Log Modules, a backup includes only
non-secret configuration and assignments. Generation history, System Logs and
Audit Logs, other Log Module destination data, and Log Module authentication or
connection credentials are outside this Application Database backup contract.

Account credential revisions, `must_change_password` flags, and temporary
credential expiry instants are restorable application state. The version-1
Restore reader accepts these fields additively: omission means revision `1`, a
false flag, and no expiry. Supplied temporary metadata must be paired and must
belong to an account with a supplied password verifier; zero revisions,
negative expiries, malformed values, and inconsistent combinations are invalid
backup content. This compatibility does not change the backup format version.

Account Public Identifiers are restorable application state. The current
forward contract for a future backup writer stores each value in its account's
`public_id` field as canonical unpadded URL-safe Base64 for the exact 16 bytes.
The Restore reader preserves each valid supplied value exactly through the
lifecycle-selected Application Database's persistence decoder and rejects
malformed, all-zero, or duplicate values without exposing them. A compatible
version-1 backup written before this field existed remains accepted when the
field is omitted; an explicitly present JSON `null` is malformed. Restore
generates a fresh independent value for each omission during normalization and
does not change the backup format version.

Account, Group, and Log Module configuration Audit Reference Identifiers are
restorable application state. The current forward contract for a future backup
writer carries their canonical rendered values. The Restore reader preserves
each valid supplied value exactly through the lifecycle-selected Application
Database's persistence decoder. A compatible version-1 backup written before
these fields existed remains accepted when the field is omitted; an explicitly
present JSON `null` is malformed rather than a legacy omission. Restore assigns
fresh independent random values to omitted fields during normalization, before
the state can be used or persisted, and never derives them from names or
`StateIdentifier` values. This reader compatibility does not change the backup
format version.

**[Restore](../../glossary.md#states-and-requests)** is exposed through a
Restore-capable Client Module after the shared lifecycle contract selects and
configures the replacement Server's Application Database and before Init
creates new application state. The person completing Restore supplies the
encrypted backup and matching private recovery key over HTTPS. The
`weavelit-server-restore` crate validates and decrypts the artifact outside the
database backend, then supplies complete normalized restored state through the
atomic restore operation.

The restore operation verifies the expected deployment identifier and eligible
Restore checkpoint before replacing application state. The Restore crate
invalidates restored sessions, re-encrypts reversibly encrypted data using the
replacement Server's own at-rest key material, preserves only the matching
public recovery key, and verifies the process-level durable acknowledgement for
the Restore-result System Log defined in the [Technical Specification](../../spec.md#logging-and-accountability).
The lifecycle crate seals the deployment record `Initialized` after the atomic
database commit and before normal routes become available. A failure after the
database commit fails closed and is classified as retained partial state without
route exposure, reconciliation, or sealing on restart. The private recovery key
and decrypted backup contents are never persisted by the Application Database
backend.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Open Questions](../../open-questions.md)
- [Glossary](../../glossary.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server Lifecycle Design](../lifecycle/lifecycle-design.md)
- [Server Init Design](../lifecycle/init/init-design.md)
- [Server Restore Design](../lifecycle/restore/restore-design.md)
- [SQLite Application Database Design](sqlite/sqlite-application-database-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Authentication Design](../authentication/authentication-design.md)
- [Temporary Password Disclosure Decision](../authentication/temporary-password-disclosure-decision.md)
- [Audit Terminal Binding Retention And Supersession Decision](../../log-modules/audit-terminal-binding-retention-decision.md)
