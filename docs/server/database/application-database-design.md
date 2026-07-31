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
storage settings. A future backend independently selects its own connection and
concurrency model behind that same contract.

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
Server.

## Deployment Record, Locator, And Operational State

The Server-local deployment record contains a unique deployment identifier and
the lifecycle state `Uninitialized`, `InitializationPending`, or `Initialized`.
The separate locator repeats that identifier, identifies the compiled-in
backend, and contains only the typed non-secret connection settings and typed
secret-file references needed to reopen the Application Database. Both remain
outside the Application Database and are persisted atomically by the Server.
Application-owned operational state and the matching deployment identifier are
persisted through the database contract.

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

Application Database selection and configuration occur through the shared
pre-operational Client Module surface. Init then selects and configures initial
Log Modules; Restore imports their application-owned configuration from the
validated backup. Selecting the same underlying technology for both does not
reuse an Application Database backend or its resources.

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
