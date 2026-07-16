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
backend composition and lifecycle. It reads the selected backend and its
host-managed bootstrap configuration, validates that configuration, constructs
the compiled-in backend, and calls it through the shared contract. A future
backend independently selects its own connection and concurrency model behind
that same contract.

## Initial Contract

The initial contract expresses Server application intent rather than storage
mechanics. It supports only these capabilities:

1. Inspect whether the Application Database is initialized.
2. Atomically persist the initial application-owned state exactly once.
3. Load the initialized application-owned state required by Server startup.

The initial write persists the complete supplied state and marks the database
initialized as one operation. If it fails, no supplied state remains and the
database stays uninitialized. A later initialization attempt returns the stable
`AlreadyInitialized` error.

The contract initially exposes these storage-neutral error categories:

```text
AlreadyInitialized
NotInitialized
InvalidState
ConfigurationInvalid
Unavailable
IntegrityFailure
```

`ConfigurationInvalid` means that host-managed backend configuration must be
corrected. `Unavailable` means an otherwise valid backend cannot currently be
opened, queried, locked, or used. `IntegrityFailure` prevents normal operation
when persisted data, schema, or migration history is damaged or incompatible.
Backend-specific error details remain private and are mapped to these safe
categories before reaching the Server.

## Bootstrap And Operational State

Host-managed bootstrap configuration identifies the compiled-in backend and
its connection settings before the Server can open the Application Database;
it remains outside the Application Database. Application-owned operational
state is persisted through the contract, including durable Server configuration
such as the listening IP address.

**[Init](../../glossary.md#states-and-requests)** is a host-local
**[Admin CLI](../../glossary.md#applications-and-interfaces)** workflow, not a
network-exposed Server function. Post-Init administration changes operational
state through its own authorized administration boundary and cannot rerun Init.

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

Application Database selection and configuration occur in a distinct host-local
Init step before Log Module selection and configuration. Selecting the same
underlying technology for both does not reuse an Application Database backend
or its resources.

## Backup And Recovery

During **[Init](../../glossary.md#states-and-requests)**, the Server creates a
backup recovery key pair. The Server persists only the public key. The
**[Host Administrator](../../glossary.md#identities-and-access)** receives the
private key once and stores it outside Weavelit. This recovery key pair is not
used to protect the Server's normal database fields and is separate from the
Server-local at-rest key material used for reversibly encrypted data.

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

Recovery is a host-local Admin CLI operation. After a replacement Server has
completed the minimal Init required to select and configure its Application
Database, the Host Administrator supplies the backup and private recovery key.
The Server verifies backup authenticity, integrity, version compatibility, and
contents before atomically replacing the target application state. It then
protects restored reversibly encrypted data using its own Server-local at-rest
key material. The private recovery key is never persisted, logged, or included
in an ordinary backup artifact.

## Related Documents

- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Open Questions](../../open-questions.md)
- [Glossary](../../glossary.md)
- [Server Architecture Design](../server-architecture-design.md)
- [SQLite Application Database Design](sqlite/sqlite-application-database-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Testing and Validation Policy](../../testing.md)
