# Server Restore Design

This document defines the Server-owned implementation boundary for
**[Restore](../../../glossary.md#states-and-requests)**. It owns encrypted backup
validation, private recovery-key handling, normalized restored-state creation,
restored-session invalidation, protected-secret re-encryption, durable Restore
result recording, and the atomic replacement of an eligible uninitialized
**[Application Database](../../../glossary.md#applications-and-interfaces)**. The
[Server Lifecycle Design](../lifecycle-design.md) owns shared database selection,
workflow arbitration, persistence anchors, startup classification, and sealing.

## Runtime And Client Module Boundary

`weavelit-server-restore` is the dedicated Server-owned crate for restoring
existing application state. The normal `weavelit-server` runtime composes it
with `weavelit-server-lifecycle` and exposes it only through Restore-capable
**[Client Modules](../../../glossary.md#applications-and-interfaces)** while the
lifecycle authority reports that Restore is eligible. Retained partial Restore
state is not eligible for Restore routing.

The Restore crate owns backup-specific request normalization, validation,
decryption, compatibility checks, and restored-state transformation. It does
not own startup classification, the deployment record, the database locator,
Application Database selection, final lifecycle sealing, or client
presentation. It receives a validated lifecycle authority and selected
database bound to the trusted replacement deployment identifier.

Every mutating Restore operation independently calls the lifecycle authority
before reading a private recovery key or backup content or causing side effects.
An initialized deployment, an Init checkpoint, an unavailable or mismatched
database, or any ineligible lifecycle combination is rejected before sensitive
input processing. Runtime route removal is an additional control rather than
the Restore operation's authority.

A Restore-capable **[Client Module](../../../glossary.md#applications-and-interfaces)**
exposes its Server-owned operations through its
**[Pre-Operational Surface](../../../glossary.md#applications-and-interfaces)**
and owns request decoding, bounded transfer, and client-specific presentation
of normalized status and errors. It passes the encrypted artifact and private
recovery key only to the Restore contract and does not decrypt, log, retain, or
interpret either. The Web UI Client Module is Restore-capable. Another Client
Module may expose Restore only by declaring that capability and using the same
Server-owned operations.

## Eligibility And Workflow Choice

Restore is available only on a genuinely uninitialized replacement Server. The
shared lifecycle contract must have selected and opened an eligible empty
Application Database before Restore accepts a backup or private recovery key.
The person may choose Init or Restore while the database remains selected and
uninitialized; creating either workflow's checkpoint makes the other workflow
unavailable.

Restore never repairs or replaces a retained initialized deployment. A missing,
malformed, unsafe, unavailable, mismatched, or integrity-failing deployment
record, locator, or configured database fails closed without exposing Restore
as a fallback. A person with sufficient host authority may remove every
persistent deployment anchor, but that host-level destruction is outside the
Restore contract.

The selected backend must be compatible with the backup. Restore does not
perform an in-place migration between Application Database technologies. The
supported backup-format and Server-version compatibility windows remain defined
by the decisions tracked in [Open Questions](../../../open-questions.md).

## Request And Sensitive Input Handling

The normalized Restore request contains a bounded encrypted backup artifact and
its matching private recovery key. The client transmits both over HTTPS, never
places the key in a URL or browser history, and does not copy the key or backup
into client-side persistent storage. The Server never returns either input.

The Restore crate treats the artifact as untrusted before and after successful
decryption. It applies the configured upload, cryptographic-work, structural,
collection, string, execution-time, concurrency, and any applicable
decompression bounds before expensive allocation or application-state
mutation. It parses structured data with the format's maintained parser and
rejects unknown required fields, duplicate identities, invalid references,
unsupported versions, unavailable required compiled-in components, and values
outside Server domain constraints.

The private recovery key is accepted only to authenticate and decrypt the
submitted backup. It is not an application identity, proof of host authority,
or authorization for another action. The Restore crate verifies that it matches
the artifact's recovery public key, retains only the corresponding public key
for future backups, and keeps the private key, unwrapped data key, and plaintext
only in bounded transient memory. Sensitive buffers are cleared through the
selected maintained cryptographic facilities when no longer needed.

If staging is required, the Server may persist only the encrypted artifact in
bounded protected temporary storage. It never persists the private recovery
key, an unwrapped data key, or decrypted backup plaintext. Staged artifacts are
not application state and are removed after success or rejection before a
checkpoint exists. An interruption that retains staging state is classified
fail-closed without automatic cleanup, resumption, or upload retry. Exact
normal-request staging mechanics remain an open design decision.

## Backup Validation And Restored State

Restore validation minimizes exposure to attacker-controlled work. After
lifecycle eligibility and transfer bounds are enforced, the Restore crate:

1. minimally parses the bounded outer envelope to identify its format version
   and declared cryptographic parameters;
2. rejects unsupported or out-of-policy parameters before cryptographic work;
3. authenticates and decrypts the envelope with the supplied recovery key under
   configured cryptographic-work limits;
4. bounds any decompression and plaintext size before parsing the authenticated
   structured contents; and
5. validates Server and source-backend compatibility, internal references,
   required components, and all domain semantics.

A failure releases transient resources without continuing to later stages,
leaves the selected database without application state, and returns only a
stable, redacted error. The exact envelope, key format, algorithms, and concrete
bounds remain the decisions recorded in Open Questions.

A valid backup supplies the application-owned state required for operation,
including account records, password verifiers, Groups and grants, enabled
component state, non-secret Log Module configurations and assignments,
protected MFA factor data, Service Connection credentials, and the recovery
public key. It does not supply the replacement deployment record, database
locator, active sessions, Server-local at-rest key, Log Module destination data,
or Log Module authentication or connection credentials. A restored remote Log
Module destination remains unusable until an authorized Administrator re-enters
its credentials through an
**[Administration Plane](../../../glossary.md#applications-and-interfaces)**.

Restore binds all normalized state to the replacement deployment identifier.
It creates no active session, accepts no session from the artifact, and ensures
that credentials from the prior deployment cannot resume a Server session. It
decrypts protected application values from the backup envelope and re-encrypts
them with the replacement Server's own at-rest key before persistence. The
private recovery key is never repurposed as an at-rest key.

## Checkpoint, Atomic Restore, And Sealing

After complete validation and before application-state mutation, the Restore
crate asks the Application Database contract to create a non-operational Restore
checkpoint under the lifecycle authority's exclusive mutation permit. The
checkpoint contains the replacement deployment identifier, the Restore workflow
discriminator, and only format-defined non-secret metadata needed for safe
classification. The lifecycle crate then advances the deployment record to
`InitializationPending` using the fail-closed ordering defined in the Server
Lifecycle Design.

The Restore crate asks the database contract to atomically replace the eligible
Restore checkpoint with the complete normalized restored state. The backend
does not receive the backup artifact or private recovery key and independently
verifies the expected deployment identifier, checkpoint kind, and one-time
state transition. A failure before commit leaves no partial application state.

The restored state carries a post-commit Restore-result obligation with
non-secret event fields. The Restore crate loads the restored System Log
assignment, receives the durable acknowledgement defined in the
[Technical Specification](../../../spec.md#logging-and-accountability) for the
Restore result, and marks that obligation complete. The record identifies
Restore, the replacement deployment identifier, time, result, and correlation
identifier without recovery keys, backup contents, restored identities, or other
protected values. The lifecycle crate seals the deployment record `Initialized`
only after the database commit and required System Log result acknowledgement.
The runtime then removes every pre-operational route, loads application state,
and enables normal authenticated operation without a restart.

If the database commit succeeds but System Log recording, sealing, or in-process
activation fails, the Server exposes no routes and fails closed. On startup,
the lifecycle crate classifies the matching initialized database and pending
deployment record as retained partial state with the stable
`lifecycle_interrupted` / `operator_redeploy_restore` diagnostic. It does not
invoke Restore, retry completion logging, seal the deployment, or reopen Init
or Restore.

## Interruption Boundary

Before a Restore checkpoint exists, a rejected request leaves the selected
database eligible for either workflow and releases transient inputs. Once a
checkpoint exists, interruption leaves retained partial state that is
non-operational. The Server does not reconcile, retry, reset, resume a staged
upload, delete a checkpoint or artifact, recreate state, seal, or expose Init
or Restore over that state.

The operator may preserve the failed root for diagnosis or evidence, or discard
it and rebuild or redeploy the replacement host. Restore then begins again only
on the new deployment with an independently retained compatible backup and its
private recovery key. Weavelit does not retain the backup or private key and
does not manage their durability.

## Concurrency And Errors

The lifecycle crate serializes Init and Restore mutation. Concurrent or stale
requests are rechecked against current trusted state; at most one workflow can
create a checkpoint and at most one atomic state replacement can succeed. A
later request returns `AlreadyInitialized`, finds another workflow pending, or
finds the Restore interface unavailable.

Restore failures use the Server's centralized typed error presentation and
compose the shared lifecycle categories. Restore-specific categories include
`backup_invalid`, `backup_incompatible`, `recovery_key_invalid`,
`restore_pending`, and `restore_failed`. Categories do not reveal whether a
guessed key partially matched, cryptographic internals, backup plaintext,
account identities, provider credentials, filesystem details, or raw dependency
errors.

## Test Evidence

`weavelit-server-restore` has direct tests for every artifact bound, malformed
and unsupported formats, cryptographic authentication and integrity failures,
wrong recovery keys, compatibility rejection, duplicate and invalid restored
state, unavailable required components, session invalidation, recovery-public-
key preservation, protected-secret re-encryption, private-key and plaintext
non-persistence, redaction, Restore-checkpoint validation, atomic rollback,
durable System Log result acknowledgement during a valid run, retained-partial-
state classification, absence of reconciliation, retry, reset, automatic
cleanup, recreation, and sealing after interruption, Restore-specific valid-run
failure classification, concurrency with Init and Restore requests, direct
invocation after sealing, and rejection before key or artifact processing.

Application Database integration tests verify the Restore checkpoint and atomic
one-time state replacement. Restore-capable Client Module contract tests verify
bounded transfer, normalized status and errors, lifecycle gating, and absence
of sensitive output. Server process tests verify interruption classification and
the in-process transition to normal operation. Web UI end-to-end tests cover
the complete Restore story and fail-closed interrupted-workflow behavior.

## Related Documents

- [Restore User Story](../../user-stories/restore-user-story.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Open Questions](../../../open-questions.md)
- [Glossary](../../../glossary.md)
- [Server Architecture Design](../../server-architecture-design.md)
- [Server Lifecycle Design](../lifecycle-design.md)
- [Server Init Design](../init/init-design.md)
- [Application Database Design](../../database/application-database-design.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
