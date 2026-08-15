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
perform an in-place migration between Application Database technologies. For
Milestone 1, Restore accepts only an exact match: the artifact's declared
outer format version and its repeated inner `format_version` must both equal
`1`, and the backup's source Application Database backend must equal the
selected backend. Anything else is rejected as `backup_incompatible`, and
Restore performs no cross-version upgrade. The compatibility window that
applies once a second backup format version or Application Database backend
exists is tracked in
[Open Questions](../../../open-questions.md#16-backup-format-and-server-version-compatibility-window).

## Compiled-In Component Inventory

A backup names the components its source deployment used. A Restore succeeds
only into a Server that can actually serve every one of them, so the runtime
reports the single inventory of what this build compiles in and every Restore is
judged against it.

For Milestone 1 that inventory is exactly:

| Component kind | Compiled in |
| --- | --- |
| **[Client Module](../../../glossary.md#applications-and-interfaces)** | `web-ui` |
| **[Log Module](../../../glossary.md#applications-and-interfaces)** | `sqlite` |
| **[MFA Module](../../../glossary.md#applications-and-interfaces)** | `totp` |
| **[Service Module](../../../glossary.md#applications-and-interfaces)** | none |
| Named operation | none |

Each name comes from the module crate that supplies it rather than from a string
literal restated in the runtime, so a compiled-in module and the inventory it is
judged by cannot drift apart. Adding a module to the build adds its name here;
nothing else may.

Content validation resolves every component a backup references against that
inventory: each Log Module configuration's module, each enrolled MFA factor's
module, each Service Connection's Service Module, and each Group grant naming a
Client Module, a Service Module, or an operation. A backup referencing anything
outside the inventory is refused as `backup_incompatible` before any checkpoint
exists.

This is an operator-visible constraint, not an internal detail. A backup taken
from a deployment that configured a Service Connection, or that enrolled an MFA
factor from any module other than `totp`, cannot be restored into this build,
because restoring it would produce a deployment whose Groups, factors, and
connections point at components that could never load. The refusal is deliberate
and is never relaxed by overstating the inventory. Comparison is exact: a
component name that differs only in case or spelling is unavailable.

## Request And Sensitive Input Handling

The normalized Restore request contains a bounded encrypted backup artifact and
its matching private recovery key. The client transmits both over HTTPS, never
places the key in a URL or browser history, and does not copy the key or backup
into client-side persistent storage. The Server never returns either input.

The Restore crate treats the artifact as untrusted before and after successful
decryption. It applies the configured upload, cryptographic-work, structural,
collection, string, execution-time, concurrency, and any applicable
decompression bounds before expensive allocation or application-state
mutation. These bounds enforce the
[Security Model](../../../security-model.md#backup-input-security-profile)'s
approved 256 MiB maximum encrypted artifact and authenticated-plaintext size,
120-second upload deadline, 300-second total request deadline, and single
concurrent Restore operation. It parses structured data with the format's
maintained parser and rejects unknown required fields, duplicate identities,
invalid references, unsupported versions, unavailable required compiled-in
components, and values outside Server domain constraints.

The collection and string bounds are enforced during deserialization, not after
it. Every backup collection deserializes at most its accepted number of typed
entries — 100,000 for a top-level collection and 256 for one Log Module
configuration's settings — and consumes any further entry as an ignored value
without building it into the wire model. Every wire string is rejected before it
is owned when it exceeds the bound of the domain value it becomes, including the
22-character encoded state identifier and the encoded ceiling of the maximum
decrypted protected value. A document that declares far more entries than the
Server accepts therefore cannot multiply the authenticated plaintext into
vector and string allocations before the bound applies; a single escaped string
may still occupy one parser buffer, which stays linear in the plaintext. An
over-full collection is still rejected as an oversized collection, and every
rejection remains `backup_invalid`.

The private recovery key is accepted only to authenticate and decrypt the
submitted backup. It is not an application identity, proof of host authority,
or authorization for another action. The Restore crate verifies that the
submitted key decrypts the artifact and that the artifact's declared recovery
public key is that key's own recipient, retains only that public key for future
backups, and keeps the private key, unwrapped data key, and plaintext only in
bounded transient memory. Sensitive buffers are cleared through the selected
maintained cryptographic facilities when no longer needed.

The Milestone 1 Restore request is held entirely in bounded transient memory.
The Server does not persist the encrypted artifact, the private recovery key,
an unwrapped data key, or decrypted plaintext to any temporary storage. The
approved bounds above — a 256 MiB maximum encrypted artifact and
authenticated-plaintext size, a 120-second upload deadline, a 300-second total
request deadline, and a single concurrent Restore operation — make this safe:
because at most one Restore may run and every input is bounded, peak resident
memory cost is bounded. Because no artifact is staged on disk, there is no
staged-artifact residue to remove and no staged-upload resumption or retry
path. An interruption before checkpoint creation releases transient memory and
leaves the selected database eligible for either workflow. On-disk staging of
the encrypted artifact may become a future option if the artifact ceiling is
raised or concurrent Restore is ever permitted, but that remains a possible
future change and not current behavior.

## Backup Envelope And Cryptography

The backup artifact begins with a fixed 8-byte magic `WLBKUP\r\n`, a big-endian
2-byte format-version field of `1`, 2 zero flag bytes, and a big-endian 8-byte
encrypted-payload-length field. An age v1 stream immediately follows this fixed
header; format version 1 defines no compression. The Restore crate implements
the [Security Model](../../../security-model.md#backup-input-security-profile)'s
approved age v1 X25519 recipient profile: X25519 key agreement with
HKDF-SHA-256 and ChaCha20-Poly1305 wrap the per-backup data key, HMAC-SHA-256
authenticates the header, and the age STREAM construction with
ChaCha20-Poly1305 encrypts the authenticated plaintext. Weavelit's backup format
defines exactly one recovery recipient, so a backup carrying no recipient
stanza, more than one stanza, an `scrypt` stanza, any other stanza type, or an
unsupported age version is rejected as `backup_incompatible` before key
agreement. The reader scans a bounded header prefix, so a header that exceeds
that bound is rejected as `backup_invalid` before its stanza type is examined;
both outcomes refuse the artifact, and the bound is never relaxed to report the
more specific category. The authenticated plaintext repeats `format_version: 1`
so the inner content is bound to the outer envelope's declared version.

A private recovery key is accepted only in its canonical age Bech32 encoding —
a lowercase `age1...` public recipient or an uppercase
`AGE-SECRET-KEY-1...` private identity — and only as exactly one canonical
line; a key with surrounding content, multiple lines, or non-canonical encoding
is rejected as `recovery_key_invalid` before decryption. This canonical
encoding is the same one Init delivers; Init's separate HMAC-based proof of
possession, defined in the
[Server Init Design](../init/init-design.md#recovery-key-delivery-and-finalization),
is not carried in the key encoding itself and has no bearing on Restore. The
Restore crate enforces only canonical syntax and cryptographic validity when a
key is submitted with a backup.

## Backup Validation And Restored State

Restore validation follows one fixed order that minimizes exposure to
attacker-controlled work and performs no state mutation until it is complete:

1. lifecycle eligibility and the exclusive Restore mutation permit;
2. configured transfer bounds (artifact size, upload deadline, and total
   request deadline);
3. the fixed outer header and its exact declared length;
4. canonical recovery-key syntax;
5. age parameter policy, rejecting unsupported or out-of-policy parameters
   before cryptographic work;
6. authenticated streaming decryption of the envelope with the supplied
   recovery key under configured cryptographic-work limits;
7. authenticated-plaintext size bounds;
8. the inner `format_version` and Server/source-backend compatibility;
9. internal references, required components, domain semantics, and the binding
   of the retained recovery public key to the submitted recovery key;
10. clearing of the private recovery key and unwrapped data key from transient
    memory, retaining only the recovery public key; and
11. checkpoint creation and atomic replacement, detailed in
    [Checkpoint, Atomic Restore, And Sealing](#checkpoint-atomic-restore-and-sealing).

A failure at any step releases transient resources without continuing to a
later step, leaves the selected database without application state, and
returns only a stable, redacted error.

Step 9's recipient binding is what makes the retained recovery public key
trustworthy. The backup declares that key in its authenticated plaintext, so a
backup encrypted to one recovery key could otherwise declare an unrelated one,
and every backup the restored deployment later produced would be encrypted to a
private key the operator may not hold. Restore therefore parses the declared key
as a canonical age recipient and requires it to equal the submitted recovery
key's own recipient. A declared key that is not a canonical recipient and one
that belongs to another identity are both rejected as `backup_invalid`,
indistinguishable from a wrong recovery key or an altered artifact, so the
rejection discloses nothing about which condition failed.

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
them with the replacement Server's own at-rest key before persistence, using the
seal-only capability defined by
[Application At-Rest Protection](../lifecycle-design.md#application-at-rest-protection).
Each value is sealed under its own protected-value kind, so a component secret,
MFA factor data, and a Service Connection credential cannot be interchanged. A
decrypted value that exceeds the plaintext bound that capability accepts is
refused during content validation, before any checkpoint exists, because it
could never be written back. The private recovery key is never repurposed as an
at-rest key.

## Checkpoint, Atomic Restore, And Sealing

Every step that can fail without leaving retained state runs before the
checkpoint exists. The Restore crate re-seals each recovered secret under the
replacement deployment's at-rest key and assembles the complete replacement
state, including the Restore-result completion obligation, while the deployment
is still uninitialized. A failure during that preparation leaves nothing to
clean up.

After complete validation and before application-state mutation, the Restore
crate asks the Application Database contract to create a non-operational Restore
checkpoint under the lifecycle authority's exclusive mutation permit. The
checkpoint contains the replacement deployment identifier, the Restore workflow
discriminator, and only format-defined non-secret metadata needed for safe
classification. The lifecycle crate then advances the deployment record to
`InitializationPending` using the fail-closed ordering defined in the Server
Lifecycle Design. Creating that checkpoint is the point of no return.

The Restore crate asks the database contract to atomically replace the eligible
Restore checkpoint with the complete normalized restored state. The backend
does not receive the backup artifact or private recovery key and independently
verifies the expected deployment identifier, checkpoint kind, and one-time
state transition. A failure before commit leaves no partial application state.

The restored state carries a post-commit Restore-result obligation with
non-secret event fields. The obligation and the System Log record it obliges are
built from the same fields by Server Observability, so the record cannot
describe something the committed state does not. The Restore crate loads the
restored System Log assignment, receives the durable acknowledgement defined in
the [Technical Specification](../../../spec.md#logging-and-accountability) for the
Restore result, and marks that obligation complete. The record identifies
Restore, the replacement deployment identifier, time, result, and correlation
identifier without recovery keys, backup contents, restored identities, or other
protected values. The lifecycle crate seals the deployment record `Initialized`
only after the database commit and required System Log result acknowledgement.
Sealing is reachable only from an acknowledged workflow, and it independently
re-reads the record and the database before writing, as defined in
[Workflow Arbitration And Sealing](../lifecycle-design.md#workflow-arbitration-and-sealing).
The runtime then removes every pre-operational route, loads application state,
and enables normal authenticated operation without a restart.

If the database commit succeeds but System Log recording, sealing, or in-process
activation fails, the Server exposes no routes and fails closed. On startup,
the lifecycle crate classifies the matching initialized database and pending
deployment record as retained partial state with the stable
`lifecycle_interrupted` / `operator_redeploy_required` diagnostic, as defined by
the classification table in the
[Server Lifecycle Design](../lifecycle-design.md). It does not
invoke Restore, retry completion logging, seal the deployment, or reopen Init
or Restore.

## Interruption Boundary

Before a Restore checkpoint exists, a rejected request leaves the selected
database eligible for either workflow and releases transient inputs. Once a
checkpoint exists, interruption leaves retained partial state that is
non-operational. The Server does not reconcile, retry, reset, delete a
checkpoint, recreate state, seal, or expose Init or Restore over that state.

The operator may preserve the failed root for diagnosis or evidence, or discard
it and rebuild or redeploy the replacement host. Restore then begins again only
on the new deployment with an independently retained compatible backup and its
private recovery key. Weavelit does not retain the backup or private key and
does not manage their durability.

## Runtime Orchestration Order

The Restore validation crate and the lifecycle typestate chain each own one half
of a Restore and do not depend on each other. The `weavelit-server` runtime owns
the only composition that joins them, and therefore owns the order below. Every
step that can fail without leaving retained state runs before the checkpoint.

1. Start the request budget, then acquire the shared lifecycle mutation lane
   without waiting. Contention returns `restore_pending`.
2. Authorize the workflow through the lifecycle authority. The permit that
   authorization returns supplies both the replacement deployment identifier and
   the selected backend the backup must match, so the authority is consulted
   before any submitted key or artifact byte is read.
3. Validate the artifact and recovery key, then release both immediately.
4. Generate the Restore-result record identifier and correlation identifier from
   operating-system randomness and read the event time.
5. Prepare the completion record and its obligation together through Server
   Observability, then build the replacement application state from the
   validated backup, the workflow permit's at-rest sealer, and that obligation.
6. Resolve the Log Module the restored backup assigns the System Log to. A
   backup naming a module this Server cannot serve fails here, while failing is
   still free.
7. Publish the fail-closed serving mode. Every connection accepted from this
   point forward serves no functional route.
8. Create the Restore checkpoint. This is the point of no return.
9. Replace the application state atomically.
10. Open the assigned System Log destination and deliver the completion record.
    The destination is opened only now, because creating a Log Module's local
    storage earlier would leave durable state a pre-checkpoint failure promised
    not to leave behind.
11. Acknowledge completion, then seal the deployment record `Initialized`.
    Sealing hands back the loaded state and the database the workflow held open,
    which the runtime retains as the operational deployment's one database
    handle rather than reopening the target it just replaced.
12. Compose the operational serving mode through the same operational composer a
    sealed startup uses, then publish it. Only a connection accepted after this
    point serves normal operation, so an in-flight fail-closed connection is
    never upgraded mid-request.

A failure before step 8 leaves the Server exactly as it was: the serving mode is
never changed, the anchor set is unmodified, and the deployment remains eligible
for either workflow. A failure at or after step 8 leaves the Server fail-closed
with its retained partial state intact. No rollback is attempted, because the
replaced state is exactly what an operator asked to discard.

No step above preflights the System Log or Audit Log destination; step 6 only
resolves the assigned System Log module's identifier, and component-availability
validation confirms each referenced Log Module is compiled into this Server
rather than proving it can commit. Restore therefore never proves the Audit Log
assignment's operability, a documented limitation described in
[Destination Preflight And Configuration Validation](../../../log-modules/log-module-design.md#destination-preflight-and-configuration-validation).

The runtime is the authority for which components a backup may reference. It
supplies the single compiled-in inventory defined in
[Compiled-In Component Inventory](#compiled-in-component-inventory) to the
validation crate, which resolves the backup's references against it.

## Two-Request Submission Protocol

The orchestration above runs behind a submission split into two requests. The
first carries the private recovery key alone; the runtime retains it, mints a
one-time ticket from operating-system randomness, and returns only the ticket.
The second presents that ticket and uploads the encrypted artifact. The recovery
key therefore never travels with the artifact, and no artifact is admitted
without a ticket this Server issued.

The runtime owns the ticket store and nothing about the wire format. It retains
only a domain-separated digest of the ticket and compares a submitted ticket
against it in constant time. At most one submission may be outstanding. Every
claim consumes the retained submission whether or not it succeeds, so a replay,
a concurrent claim, a wrong ticket, and an expired ticket all destroy the
retained recovery key rather than leaving it available for another attempt. An
outstanding submission expires on its own schedule after the approved upload
deadline, capped at what remains of the total request deadline the first request
started, so an abandoned submission does not leave a recovery key resident for
the listener's lifetime.

Eligibility is re-checked at request time on both requests, because the listener
snapshots the whole serving surface when it accepts a connection: a connection
accepted while a Restore was still eligible keeps a router mounting both routes
even after a checkpoint exists. Route absence is an additional control, not the
authority. The
[Web UI Pre-Operational Restore Surface](../../../client-modules/web-ui/pre-operational-restore-design.md)
owns the routes, schemas, headers, bounds, and rejection contract.

## Concurrency And Errors

The lifecycle crate serializes Init and Restore mutation. Concurrent or stale
requests are rechecked against current trusted state; at most one workflow can
create a checkpoint and at most one atomic state replacement can succeed. A
later request returns `AlreadyInitialized`, finds another workflow pending, or
finds the Restore interface unavailable.

Restore failures use the Server's centralized typed error presentation and
compose the shared lifecycle categories. Restore-specific categories include
`recovery_key_invalid` for malformed key syntax; `backup_invalid` for a wrong
key, an altered artifact, or any other authentication failure, which remain
indistinguishable from one another; `backup_incompatible` for an unsupported
format, backend, or component; `restore_pending` for a concurrent or stale
request; and `restore_failed` for a timeout, storage failure, or other
internal failure. Categories do not reveal whether a guessed key partially
matched, cryptographic internals, backup plaintext, account identities,
provider credentials, filesystem details, or raw dependency errors.

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
Fixture-based tests use immutable raw `.wlitbackup` files, their canonical
private-key line, the expected decrypted plaintext, and a canonical JSON
manifest recording each fixture's exact byte lengths and SHA-256 digests. Every
fixture is produced by one deterministic generator, run with
`cargo run --example generate-restore-fixtures -p weavelit-server-restore`, and
a test regenerates the whole set and compares it byte for byte with the
committed bytes and the manifest, so a fixture cannot be hand-edited.

Two valid backups are committed, both sealed to the same committed recovery key
and differing only in the components they reference:

| Fixture | References | Purpose |
| --- | --- | --- |
| `valid.wlitbackup` | `web-ui`, `sqlite`, `totp`, `zendesk` | The canonical valid artifact, and the backup a build compiling in fewer components must refuse. |
| `valid-web-ui-sqlite.wlitbackup` | `web-ui`, `sqlite` | The backup whose references match this build's [compiled-in inventory](#compiled-in-component-inventory) exactly, so it is the artifact an end-to-end Restore of the real Server actually restores. |

Each valid artifact commits its expected decrypted plaintext alongside it.
Negative fixtures mutate exactly one property of `valid.wlitbackup` at a time so
each failure path is independently attributable. Because those fixtures are
produced by a second implementation in this repository, the age v1 reader is
additionally validated against 110 external known-answer vectors vendored from
the C2SP Community Cryptography Test Vectors for age; the
[Server Architecture Design](../../server-architecture-design.md) records their
provenance, license, and the pinned outcome of every one.

Both valid artifacts restore one `administrator` account, and its committed
password verifier must be one the Server actually accepts and one a known
password actually verifies against: a structurally valid but unusable
placeholder would satisfy every content rule above while making sign-in
permanently impossible, and would defeat the purpose of an end-to-end sign-in
test. The verifier is therefore derived, not written down. The fixture
generator's test harness (`tests/support/mod.rs`) derives it at the current
approved Argon2id profile from a fixed salt and the documented fixture
password held in its `FIXTURE_ADMINISTRATOR_PASSWORD` constant, using the
Server's own `weavelit-server-authentication` crate as a test-only dependency
so the verifier cannot drift from the
[approved profile](../../authentication/authentication-design.md#password-hashing)
or its
[allowlist](../../authentication/authentication-design.md#accepted-verifier-profiles).
Because the password is shared plaintext embedded in every generated backup,
regenerating it necessarily rewrites the derived bytes of every fixture that
embeds it, including every negative fixture derived from `valid.wlitbackup`;
that is expected and is not evidence of an unrelated change. `tests/credentials.rs`
reads the verifier back through the production Restore reader, rather than
from the plaintext expectation beside it, and asserts the real
`PasswordAuthenticator` returns `Verified { replacement: None }` for the
documented fixture password against both valid artifacts, pinning the verifier
to the *current* approved profile rather than merely an accepted one, and
denies every other tried password.

Application Database integration tests verify the Restore checkpoint and atomic
one-time state replacement. Restore-capable Client Module contract tests verify
bounded transfer, normalized status and errors, lifecycle gating, and absence
of sensitive output. Server process tests verify interruption classification and
the in-process transition to normal operation. Web UI end-to-end tests cover
the complete Restore story, a real sign-in to the restored account, that
session's persistence across a Server restart, and fail-closed
interrupted-workflow behavior.

`weavelit-server` tests drive the runtime orchestration directly against the
committed backup fixtures. They prove that a valid Restore activates the
operational surface for a newly accepted connection while every pre-operational
route becomes absent, that the committed state survives a fresh startup
classification unchanged, that the Restore result is durable in the Log Module
the restored backup itself assigns, that a wrong recovery key and a malformed
artifact both fail with the serving mode and the anchor set untouched, that a
failure after the checkpoint stays fail-closed across a restart with no
rollback, and that no rendered failure discloses recovery material or backup
plaintext.

The compiled-in inventory is proven at that layer rather than assumed. One test
drives the complete two-request protocol against the real inventory the composed
listener reports, using the fixture whose references match it, so a Restore that
the released binary could not perform fails in the Rust suite rather than only
in the browser suite. A second test submits `valid.wlitbackup` to that same real
inventory and asserts it is refused as `backup_incompatible` with the serving
mode, the anchor set, and the lifecycle record untouched, then restores the
matching fixture on the same deployment so the refusal is attributable to the
component check alone. A supplied fuller inventory remains in use only for the
tests that exercise other behavior with the canonical fixture.

## Related Documents

- [Restore User Story](../../user-stories/restore-user-story.md)
- [Technical Specification](../../../spec.md)
- [Security Model](../../../security-model.md)
- [Open Questions](../../../open-questions.md)
- [Glossary](../../../glossary.md)
- [Server Architecture Design](../../server-architecture-design.md)
- [Server Lifecycle Design](../lifecycle-design.md)
- [Server Init Design](../init/init-design.md)
- [Web UI Pre-Operational Restore Surface](../../../client-modules/web-ui/pre-operational-restore-design.md)
- [Application Database Design](../../database/application-database-design.md)
- [Authentication Design](../../authentication/authentication-design.md)
- [Testing and Validation Policy](../../../testing.md)
- [Log Module Design](../../../log-modules/log-module-design.md)
