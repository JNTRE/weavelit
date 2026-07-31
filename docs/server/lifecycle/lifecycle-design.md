# Server Lifecycle Design

This document defines the shared Server-owned lifecycle boundary used before
the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** enters
normal operation. It owns startup classification, the deployment record,
**[Application Database](../../glossary.md#applications-and-interfaces)** selection
and locator persistence, workflow arbitration, mutation serialization, and
irreversible sealing. The [Server Init Design](init/init-design.md) and
[Server Restore Design](restore/restore-design.md) own their distinct application-state
workflows.

## Crate And Runtime Boundary

`weavelit-server-lifecycle` is the internal base crate shared by
**[Init](../../glossary.md#states-and-requests)** and
**[Restore](../../glossary.md#states-and-requests)**. It owns the trusted lifecycle
state and operations that both workflows require. The Init and Restore crates
depend on it but do not depend on each other.

The `weavelit-server` runtime supplies the compiled-in Application Database
backend catalog, asks the lifecycle crate to classify trusted state, and exposes
only the routes permitted by that classification. Runtime routing is an
additional control rather than the authority for a mutation. Every mutating
lifecycle, Init, and Restore operation independently asks the lifecycle crate to
open and validate the deployment record, locator, selected database, deployment
identifier, and current workflow eligibility before reading request secrets or
backup content or causing side effects.

The lifecycle crate does not create initial users, configure initial Log
Modules, generate or accept private recovery keys, interpret backup contents,
or implement client presentation. Those responsibilities remain in their
workflow and Client Module boundaries.

## Deployment Record And Database Locator

On a first startup where both the deployment record and database locator are
absent, the lifecycle crate creates a protected deployment record atomically
with a cryptographically random deployment identifier and `Uninitialized`
state before any pre-operational route is exposed. A locator without a
deployment record fails closed.

The deployment record is a versioned, restrictive, non-symlink Server-local
file. The separate locator contains the same deployment identifier, selected
backend identifier, typed non-secret connection settings, and typed secret-file
references required to reopen the selected database. The lifecycle crate writes
and synchronizes each file through a unique temporary file and atomic
replacement. Neither file accepts inline secrets, environment interpolation,
or a caller-selected path.

The lifecycle crate accepts a referenced secret file only when the opened
object is a bounded regular non-symlink file without group or world access. It
verifies the opened object before reading UTF-8 content, trims at most one final
newline, and never logs the secret, its contents, or its path. Package and
deployment policy determine the permitted owner, and the Server process must be
able to read the file.

The deployment record has only these states:

```text
Uninitialized
InitializationPending
Initialized
```

The selected database distinguishes an Init checkpoint from a Restore
checkpoint while reporting both as non-operational pending state. Each pending
checkpoint carries the deployment identifier, a workflow discriminator, and
only the non-secret workflow metadata required for safe retry or reconciliation.
Workflow crates own the meaning and validation of their checkpoint metadata.

No supported operation transitions an `Initialized` record to another state.
`InitializationPending` may return to `Uninitialized` only through an explicit
workflow recovery operation after the lifecycle crate verifies that the
selected database contains no application state. Only the workflow identified
by the checkpoint may request that reset; the other workflow remains
unavailable. The lifecycle crate performs the shared record transition, and the
owning workflow discards its checkpoint and any workflow-owned temporary
artifacts according to its design. Complete removal of every persistent
deployment anchor by sufficient host authority may appear to the Server as a
new installation; preventing or detecting that destruction belongs to
deployment access control and monitoring.

## Startup Classification

Package installation supplies only host and process configuration needed to
start the Server, including its HTTPS listener, TLS material, and protected
Server state directory. The lifecycle crate classifies every startup before the
runtime exposes an application route:

| Deployment record | Locator and database state | Server behavior |
| --- | --- | --- |
| Absent | Locator absent | Create an `Uninitialized` record, then expose restricted pre-operational status without a database selection. |
| Absent | Locator present | Fail startup closed without exposing Init or Restore. |
| `Uninitialized` | Locator absent | Expose restricted pre-operational status without a database selection. |
| `Uninitialized` | Matching locator and uninitialized database | Expose restricted pre-operational status with the selected database. |
| `Uninitialized` | Matching locator and Init checkpoint | Advance the record to `InitializationPending`, then expose only Init reconciliation. |
| `Uninitialized` | Matching locator and Restore checkpoint | Advance the record to `InitializationPending`, then expose only Restore reconciliation. |
| `Uninitialized` | Matching locator and initialized database | Fail startup closed because the combination violates finalization ordering. |
| `InitializationPending` | Matching locator and Init checkpoint | Expose only Init reconciliation and finalization. |
| `InitializationPending` | Matching locator and Restore checkpoint | Expose only Restore reconciliation and finalization. |
| `InitializationPending` | Matching locator and initialized database | Complete workflow-specific post-commit reconciliation and seal the record before normal operation. |
| `Initialized` | Matching locator and initialized database | Load application state and start normal authenticated operation. |
| Any existing record | Missing required locator, identifier mismatch, unexpected state combination, unsafe or malformed state, unavailable database, or integrity failure | Fail startup closed without exposing Init or Restore. |

An unavailable, missing, malformed, unsafe, mismatched, or integrity-failing
configured database never causes the Server to expose a pre-operational
workflow as fallback recovery. An `InitializationPending` deployment is
non-operational and exposes only the workflow identified by its checkpoint. It
does not enable login, administration, or normal client functions.

## Application Database Selection

The lifecycle crate presents the runtime-supplied backend catalog and each
backend's typed, non-secret connection fields through Client Modules that
declare an applicable pre-operational capability. A client selects a backend
and submits connection settings and any typed secret-file references. It never
selects the locator path or supplies locator contents.

Selection validates the common request structure, asks the selected backend to
validate its settings, safely resolves only the references required to connect,
opens the target, and inspects its trusted state. An initialized, pending,
unavailable, mismatched, or integrity-failing target is not eligible. This
preflight occurs before Init accepts an Administrator password or Log Module
credential and before Restore accepts a backup or private recovery key.

After successful preflight, the lifecycle crate writes the protected locator
and opens the selected database without a process restart. The client may
replace the selection with another eligible database before either workflow
creates a pending checkpoint. Replacement is fully preflighted before atomic
locator replacement and does not delete artifacts at the previous destination.
Database selection becomes immutable when a pending checkpoint exists and
remains permanently immutable after sealing.

## Workflow Arbitration And Sealing

Init and Restore are mutually exclusive consumers of one selected uninitialized
database. The lifecycle crate grants an exclusive workflow mutation permit only
after rechecking current trusted state. Creating either workflow checkpoint
makes the other workflow unavailable. Concurrent or stale requests are
rechecked while serialized; at most one workflow can commit application state.

The lifecycle progression is:

```text
Uninitialized -> DatabaseSelected -> InitializationPending -> Initialized
```

The deployment record remains `Uninitialized` through `DatabaseSelected`.
Before either workflow commits application state, its database checkpoint and
the deployment record become `InitializationPending` using crash-safe ordering.
The Application Database performs the workflow's complete state replacement
atomically and remains the final one-time guard. After that commit, the
lifecycle crate seals the deployment record `Initialized`. Only after the seal
is durable may the runtime remove all pre-operational routes, load application
state, and enable normal authenticated operation in the same process.

If database state commits but sealing or in-process activation fails, the
runtime exposes no routes and fails closed. On restart, the lifecycle crate
verifies the matching initialized state, completes any workflow-specific
post-commit requirement, seals the record, and only then permits normal
operation. The lifecycle crate does not interpret a workflow-specific
obligation: it invokes the workflow crate that created it and seals only after
that crate reports durable completion. Init currently has no post-commit
obligation after its atomic state write; Restore owns its required durable Audit
Log result. Neither Init nor Restore is exposed again.

## Errors And Sensitive Output

Lifecycle failures use the Server's centralized typed error presentation.
Client Modules receive actionable, redacted, machine-readable categories such
as `already_initialized`, `preoperational_unavailable`,
`configuration_invalid`, `deployment_state_invalid`,
`secret_reference_unsafe`, `secret_reference_unavailable`,
`storage_unavailable`, and `storage_integrity_failure`. Raw Rust, dependency,
SQL, filesystem, secret-path, and operating-system details never reach clients
or logs.

## Test Evidence

`weavelit-server-lifecycle` has direct tests for every startup classification,
versioned record and locator parsing, restrictive and atomic local writes,
secret-reference safety, deployment-identifier matching, backend selection and
replacement, mutation serialization, workflow exclusivity, every cross-store
crash point, seal reconciliation, direct invocation after sealing, rejection
before secret or backup reading, redaction, and fail-closed missing, malformed,
mismatched, unavailable, and integrity-failing state.

Application Database integration tests verify workflow checkpoint
discrimination, atomic one-time state replacement, and deployment-identifier
enforcement. Server process tests verify route gating and each transition from
restricted pre-operational state to normal operation.

## Related Documents

- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server Init Design](init/init-design.md)
- [Server Restore Design](restore/restore-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Application Database Design](../database/application-database-design.md)
