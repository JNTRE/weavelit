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
2. Atomically create, reconcile, or discard a non-operational Init or Restore
   checkpoint containing the deployment identifier, workflow discriminator,
   and only the non-secret metadata defined by that workflow's contract.
3. Atomically replace an eligible Init checkpoint with complete new application
   state bound to that deployment identifier exactly once.
4. Atomically replace an eligible Restore checkpoint with complete validated
   restored application state bound to that deployment identifier exactly once.
   The backend receives normalized state and never parses, decrypts, stages, or
   validates a backup artifact or private recovery key.
5. Load the initialized application-owned state and deployment identifier
   required by Server startup.

The Rust contract is synchronous and object-safe. `ApplicationDatabase`
requires a movable backend and takes exclusive mutable access for every call;
it does not require a backend to be safe for concurrent shared access. The
lifecycle boundary serializes workflow mutation and decides where blocking
storage calls execute when it composes the backend. This keeps runtime and
executor dependencies out of the persistence contract while allowing a future
backend to implement the same operations with its own connection model.

`DeploymentIdentifier` is an opaque 16-byte value. The lifecycle boundary
generates it cryptographically, and the contract rejects the reserved all-zero
representation. The contract exposes only its binary representation and
redacts it from diagnostic formatting. A migrated database with no checkpoint
or application state is unbound. Creating the first checkpoint binds the
database to its deployment identifier; discarding that matching checkpoint
returns the otherwise empty database to unbound state. Pending and initialized
state always carry the persisted binding.

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
checkpoint. Reconciliation verifies that the same deployment identifier,
workflow, and metadata remain durable without changing them. Discard receives
the expected deployment identifier and workflow and removes only that matching
pending checkpoint. These operations expose no transaction, query, migration,
path, connection, or backend-selection mechanism.

Each final write persists the complete supplied state and marks the database
initialized as one operation. A pending checkpoint is not application state and
permits only the workflow identified by its discriminator. An Init checkpoint
prevents a new key pair from being generated while the requesting client proves
possession of the delivered private key. A Restore checkpoint prevents Init or
a second Restore attempt from replacing its in-progress workflow. If a final
write fails, the checkpoint remains available for that same workflow to resume
or reset safely according to its owning design. Every later Init or Restore
attempt returns the stable `AlreadyInitialized` error.

The backend compares the expected deployment identifier on every checkpoint,
discard, finalization, load, and restore operation. It rejects a mismatch before
changing state. The identifier is an integrity binding between this database
and the Server-local deployment record and locator; it is not an authentication
credential or secret.

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

## Deployment Record, Locator, And Operational State

The Server-local deployment record contains a unique deployment identifier and
the lifecycle state `Uninitialized`, `InitializationPending`, or `Initialized`.
The separate locator repeats that identifier, identifies the compiled-in
backend, and contains only the typed non-secret connection settings and
Server-encrypted secret connection values needed to reopen the Application
Database. Both remain outside the Application Database and are persisted
atomically by the Server. The locator never contains plaintext secrets or a
caller-supplied path or file reference. Application-owned operational state and
the matching deployment identifier are persisted through the database contract.

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
a fallback. Cross-store ordering, sealing, and crash reconciliation are defined
in the [Server Lifecycle Design](../lifecycle/lifecycle-design.md).

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
sessions, which are invalidated on restore. System Logs and Audit Logs are
separate Log Module data and are outside this Application Database backup
contract.

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
public recovery key, and verifies durable Restore-result System Log recording.
The lifecycle crate seals the deployment record `Initialized` after the atomic
database commit and before normal routes become available. A failure after the
database commit fails closed and is reconciled before route exposure on the
next startup. The private recovery key and decrypted backup contents are never
persisted by the Application Database backend.

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
