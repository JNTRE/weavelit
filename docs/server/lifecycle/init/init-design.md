# Server Init Design

This document defines the Server-owned implementation boundary for
**[Init](../../../glossary.md#states-and-requests)**. It defines how the normal Server
runtime exposes a restricted initialization contract, handles new-state
requests and recovery-key delivery, and atomically creates initial application
state. The shared lifecycle, selected
**[Application Database](../../../glossary.md#applications-and-interfaces)** locator,
deployment record, and transition into normal operation belong to the
[Server Lifecycle Design](../lifecycle-design.md).
**[Client Modules](../../../glossary.md#applications-and-interfaces)** own transport
and presentation only; they cannot create alternative initialization behavior.

## Runtime And Client Module Boundary

`weavelit-server-init` is the dedicated Server-owned crate for initialization.
The normal `weavelit-server` runtime composes it with
`weavelit-server-lifecycle` and uses it only to process
**[Init](../../../glossary.md#states-and-requests)** requests. The Init crate owns
normalized new-state requests, Server semantic validation, initial
recovery-key generation and delivery, and the atomic initial-state transition.
It does not own startup classification, the deployment record, the selected
database locator, Application Database selection, or final lifecycle sealing.

The lifecycle crate exposes shared pre-operational status and Application
Database selection. The Init crate exposes Server-owned operations for
preparing initial recovery-key delivery and invoking the final `InitializeServer`
use case. These operations form the new-state workflow; none is an alternative
path that can create complete application state independently.

Every mutating Init operation independently calls the lifecycle authority,
which opens and validates the deployment record and selected database before
the Init crate reads request secrets or changes database state. The Init crate
derives lifecycle state and the deployment identifier only from that authority
and never accepts either from a caller. An `Initialized` record rejects every
mutating operation before side effects, even if the operation is invoked
directly inside the process after a routing or composition defect. The runtime
lifecycle gate and Client Module route removal are additional controls, not
the Init operation's authority.

An Init-capable **[Client Module](../../../glossary.md#applications-and-interfaces)**
may translate requests from its
**[Pre-Operational Surface](../../../glossary.md#applications-and-interfaces)**
into these operations while the Server is uninitialized. It owns request
decoding, client-specific interaction, and presentation of normalized results.
It may also expose shared lifecycle status and database selection through the
lifecycle contract, but it has no direct Application Database, driver,
locator-file, database-file, credential-storage, or lifecycle-state access. The
Web UI Client Module is Init-capable. Another Client Module may expose Init only
by declaring that capability and using the same Server-owned operations.

The runtime uses the lifecycle classification before dispatching any client
request. It exposes Init operations only while the lifecycle authority reports
that the new-state workflow is eligible, and it rejects every normal application
function during that period. Retained partial Init state is never eligible for
Init routing. After successful Init or Restore, it exposes no Init operation and
serves only normal authenticated functions. A client-supplied state or route
cannot alter that gate.

## Shared Lifecycle Dependency

The [Server Lifecycle Design](../lifecycle-design.md) owns startup classification,
the deployment record, the database locator, Application Database selection,
workflow arbitration, mutation serialization, and sealing. Init receives only a
validated lifecycle authority and selected database bound to the trusted
deployment identifier. It never accepts a deployment identifier, lifecycle
state, locator path, or database handle from a client.

Lifecycle preflight rejects an initialized, pending, unavailable, mismatched, or
integrity-failing database before Init accepts the first
**[Administrator](../../../glossary.md#identities-and-access)** password,
**[Log Module](../../../glossary.md#applications-and-interfaces)** credentials, or
other application secrets. Database selection may change only before Init
creates its pending checkpoint. Init persists all user-selected application
configuration other than the minimum database locator during finalization.

## Request And Secret Handling

The final normalized `InitializeServer` request contains the first local
**[Human User](../../../glossary.md#identities-and-access)**, their password, initial
**[Log Module](../../../glossary.md#applications-and-interfaces)** configurations,
explicit **[System Log](../../../glossary.md#applications-and-interfaces)** and
**[Audit Log](../../../glossary.md#applications-and-interfaces)** assignments, and
the recovery-key proof required for the pending checkpoint. The use case creates
the system-defined **[Administrators Group](../../../glossary.md#identities-and-access)**
and adds the first user without accepting client-defined grants. The request
does not duplicate the selected database connection configuration.

Client applications submit user-supplied secrets over HTTPS and must not place
them in URLs or persistent client storage.
**[Client Modules](../../../glossary.md#applications-and-interfaces)** pass secret
values unchanged to the Init crate and do not log or retain them. Client-side
validation may improve usability but is never authoritative; the Init crate
validates every value, never returns it, and persists it only in its intended
protected representation, such as a password verifier or encrypted credential.

Database connection secrets are submitted only through backend-declared typed
fields and terminate in the shared lifecycle contract's protected credential
handling. Init and its Client Modules never accept a database, locator, or
credential-storage path or file reference from a client.

## Recovery-Key Delivery And Finalization

Recovery-key delivery uses the
**[Application Database](../../../glossary.md#applications-and-interfaces)**'s
non-operational `InitializationPending` checkpoint so the Server never becomes
initialized before the requesting client proves possession of the private key:

1. Recovery-key preparation requires a selected, uninitialized Application
   Database. Under the lifecycle authority's exclusive mutation permit, the
   Init crate generates the recovery key pair and asks the database contract to
   atomically record an Init checkpoint containing the deployment identifier,
   public key, and a unique delivery nonce. The lifecycle crate advances the
   deployment record to `InitializationPending`. Init returns the private key
   once over HTTPS only after both writes complete their configured valid-run
   commit paths. The private key exists only transiently in Server memory and
   response handling and is never persisted.
2. The client saves the private key outside Weavelit and derives the
   key-format-defined proof of possession for the delivery nonce. It submits
   that proof, but not the private key, with the complete normalized
   `InitializeServer` request.
3. The Init crate obtains the reopened selected database from the lifecycle
   authority, verifies that the deployment identifiers, checkpoint, and proof
   match, validates the complete request, verifies the
   **[Log Module](../../../glossary.md#applications-and-interfaces)** assignments
   and their ability to provide the durable acknowledgement defined in the
   [Technical Specification](../../../spec.md#logging-and-accountability) for
   each assigned log type, and atomically replaces the checkpoint with complete
   initialized application state bound to the deployment identifier. The
   committed state includes the non-secret Init completion-event fields as a
   pending obligation.
4. The Init crate loads the committed
   **[System Log](../../../glossary.md#applications-and-interfaces)** assignment,
   receives the required durable acknowledgement for the successful Init result,
   and marks the completion obligation satisfied. The record identifies Init,
   the deployment identifier, time, result, and correlation identifier without
   passwords, recovery-key material, or other submitted secrets.
5. After the completion result is acknowledged, the lifecycle crate atomically
   seals the deployment record `Initialized`. Only after the seal's configured
   valid-run commit path completes does the runtime close every pre-operational
   gate, load the committed application state, and enable normal authenticated
   operation in the same process.

Proof of possession confirms that the requesting client retained the delivered
key long enough to finalize Init; safeguarding the downloaded private key
outside Weavelit remains the responsibility of the person completing Init. The
Server never redisplays a private key associated with an existing checkpoint.

If delivery, validation, final persistence, System Log completion,
deployment-record sealing, or in-process activation is interrupted after the
checkpoint exists, the Server exposes no routes and fails closed. On restart,
the shared lifecycle classifies the retained state with the stable
`lifecycle_interrupted` / `operator_redeploy_new` diagnostic; it does not
redisplay a key, resume Init, retry a request or completion log, reset the
checkpoint, delete retained state, or seal the deployment. The operator may
preserve the failed root for diagnosis or evidence, or discard it and redeploy
before beginning a new Init. An external Log Module destination may retain a
non-application artifact created during validation; cleanup remains that
module's design and is not a lifecycle recovery action.

## Concurrency, Lifecycle, And Errors

The lifecycle crate serializes deployment-record and locator mutation across
**[Init](../../../glossary.md#states-and-requests)** and Restore. Recovery-key
preparation and finalization run only under its exclusive workflow mutation
permit. The
**[Application Database](../../../glossary.md#applications-and-interfaces)**'s atomic
state transitions remain the final one-time guard. Concurrent or stale requests
are rechecked against current trusted state; at most one workflow can commit,
and every later attempt returns `AlreadyInitialized` or finds the Init
interface unavailable.

Init uses the shared lifecycle progression:

```text
Uninitialized -> DatabaseSelected -> InitializationPending -> Initialized
```

The deployment record remains `Uninitialized` through `DatabaseSelected`.
Database selection may change only before `InitializationPending`. Failure
before locator persistence leaves the deployment record `Uninitialized`; an
interruption after a checkpoint exists leaves the retained state
non-operational. The `Initialized` transition is irreversible through every
supported interface. Init never partially exposes normal application behavior.

All Init failures use the Server's centralized typed error presentation and
compose the shared lifecycle categories defined in the
[Server Lifecycle Design](../lifecycle-design.md).
**[Client Modules](../../../glossary.md#applications-and-interfaces)** receive
actionable, redacted, machine-readable categories. Init-specific categories are
`recovery_key_confirmation_required`,
`recovery_key_confirmation_invalid`, and `initialization_failed`. Raw Rust,
dependency, cryptographic, SQL, filesystem, and operating-system errors never
reach clients or logs.

## Test Evidence

`weavelit-server-init` has direct tests for normalized-request validation,
recovery-key generation, one-time delivery, proof, Init-checkpoint validation,
atomic new-state creation, durable System Log completion during a
valid run, retained-partial-state classification, absence of restart retry,
reset, deletion, recreation, reconciliation, and sealing, redaction, rollback,
concurrency under a lifecycle mutation permit, direct invocation of every
mutating entry point after sealing, rejection before secret reading or side
effects, and the one-time `AlreadyInitialized` guard.

**[Application Database](../../../glossary.md#applications-and-interfaces)**
integration tests verify Init-checkpoint transitions and atomic one-time
new-state persistence. Init-capable
**[Client Module](../../../glossary.md#applications-and-interfaces)** contract tests
verify one-time private-key delivery, finalization, normalized errors, rejection
of normal functions before Init, and rejection of Init after completion. Shared
lifecycle tests own status, database selection, startup classification, and seal
interruption classification. Server process tests verify the in-process
transition to normal operation.
**[Web UI](../../../glossary.md#applications-and-interfaces)** end-to-end tests cover
the complete first-launch workflow and fail-closed handling of an interrupted
delivery.

## Related Documents

- [Init User Story](../../user-stories/init-user-story.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Server Architecture Design](../../server-architecture-design.md)
- [Server Lifecycle Design](../lifecycle-design.md)
- [Application Database Design](../../database/application-database-design.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
- [Development Container Design](../../../containers/dev/development-container-design.md)
- [Production Container Design](../../../containers/prod/production-container-design.md)
- [Testing and Validation Policy](../../../testing.md)
- [Milestone 1](../../../plan/milestones/milestones.md#milestone-1-core-server-application)
