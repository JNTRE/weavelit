# Server Audit Log Design

This document owns the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
producer contract for **[Audit Logs](../../glossary.md#applications-and-interfaces)**.
It defines how a consequential authenticated action becomes bounded,
pre-redacted attempt, completion, or correction records before delivery. The
shared complete-record envelope, destination acknowledgement, classification
catalog implementation, and destination storage remain defined by the
[Log Module Design](../../log-modules/log-module-design.md).
This document does not define Audit Log retrieval, query, export, client
presentation, destination-specific storage, or destination redaction.

`weavelit-server-audit` implements this producer boundary. Its first consuming
**[Administration Plane](../../glossary.md#applications-and-interfaces)**
workflows are the Server-owned, transport-independent TOTP MFA Module
enablement change and internal Log Module configuration change. Other
administration mutations remain future work.

## Ownership And Invariants

The Server owns authorization, mutation sequencing, and the decision to commit
a consequential mutation. The `weavelit-server-audit` component constructs and
pre-redacts the Audit body, using only closed typed facts and the
Server-supplied workflow correlation identifier, then supplies it to the
`weavelit-server-log` contract. A Log Module receives only the complete
immutable record; it must not redact, enrich, reinterpret, or read Application
Database state.

Every consequential authenticated application action must be attributable to
the authenticated principal and must produce the accountability records
defined below. Init and Restore are pre-operational lifecycle actions and
produce System Logs, not Audit Logs. Operational diagnosis remains in the
Server Observability boundary.

## Bounded Record Contract

The **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
generates the opaque `record_id` and UTC Unix-millisecond
`event_time` in trusted Server context. The owning request workflow already
has the Server-generated `correlation_id` used by the API response and any
related System Log; it supplies that value to the producer. Audit construction
does not create a new correlation identifier. A non-request workflow supplies
its owning workflow-generated correlation identifier. The producer constructs
the following pre-redacted Audit body and complete record:

| Field | Required contract |
| --- | --- |
| `record_id` | Server-generated, opaque, nonzero 16-byte identifier. Each attempt, completion, or correction receives a fresh identifier; a caller, producer input, or destination must not choose or reuse it. |
| `event_time` | Server-generated UTC Unix time in milliseconds. |
| `phase` | Exactly `attempt`, `completion`, or `correction`. An attempt records accepted intent before mutation; a completion or correction records an authoritative outcome. |
| `result` | Absent for `attempt`. Exactly `success` or `failure` for `completion` and `correction`. |
| `attempt_record_id` | Absent for `attempt`. Every newly constructed `completion` or `correction` requires the immutable opaque 16-byte `record_id` capability minted from its precise prior Attempt. |
| `correlation_id` | Non-empty UTF-8 value bounded by `MAX_CORRELATION_ID_BYTES` (64 bytes). |
| `classification` | One value from the closed typed Audit catalog, bounded by `MAX_AUDIT_CLASSIFICATION_BYTES` (128 bytes). |
| `principal` | Structured human or automation principal, non-empty and bounded by `MAX_AUDIT_PRINCIPAL_BYTES` (256 bytes). |
| `responsible_owner` | Required for an automation principal and absent for a human principal; bounded by `MAX_AUDIT_RESPONSIBLE_OWNER_BYTES` (256 bytes). |
| `action` | Non-empty safe action summary bounded by `MAX_AUDIT_ACTION_BYTES` (128 bytes). |
| `target` | Non-empty safe target summary bounded by `MAX_AUDIT_TARGET_BYTES` (1024 bytes). |
| `detail` | Non-empty safe outcome/context summary bounded by `MAX_AUDIT_DETAIL_BYTES` (4096 bytes). |

All bounds are UTF-8 byte bounds, not character counts. The correlation
identifier and all Audit body fields together must not exceed the shared
`MAX_RECORD_PAYLOAD_BYTES` limit of 8 KiB. Empty, malformed, or oversized
values are rejected before complete-record construction. The producer must not
truncate, hash, retain source values for later reconstruction, construct a
partial or replacement record, or pass rejected input to a destination.

The record's debug and error representations are payload-free. A record
identifier is not a replacement for valid body content and must not be reused
with different content. Related records reuse the owning workflow's correlation
identifier while retaining distinct record identifiers. Audit construction
does not accept a standalone Application Database `StateIdentifier`, name, or
raw account, Group, or Log Module configuration string. Account, Group, and Log
Module configuration fields consume the database contract's typed persisted
Audit projections. Account and Group targets render only their
`audit_reference()` value as `account:ar-...` or `group:ar-...`; the
configuration-change target renders the canonically ordered affected values as
`log-configuration:ar-...`. None reads or serializes the projection's state
identifier. The producer renders every other safe target from its closed typed
input. The shared envelope remains the schema authority for the typed phase,
terminal result, and Attempt link invariant.

`AttemptRecordId` is an opaque typed capability. Only a complete Audit Attempt
can mint it; callers cannot convert or clone an arbitrary `RecordId` into an
Attempt link. Public completion and correction constructors consume this
capability together with a `LogResult` and reject a different correlation
identifier, reuse of the Attempt record identifier, or an event time before the
Attempt, so a newly constructed terminal record without one precise prior
same-correlation Attempt is unrepresentable. The destination separately proves
that the referenced Attempt was already persisted before accepting the terminal
record.

## Construction, Redaction, And Delivery

The **[Audit Log](../../glossary.md#applications-and-interfaces)** producer
receives only the validated facts needed to describe the action.
It creates fixed or allowlisted summaries for `classification`, `action`,
`target`, and `detail`; it does not serialize a request, response, database
row, exception, or arbitrary user-provided value into an Audit record. The
following values must never appear in an Audit Log, including inside a summary:

- temporary passwords, password verifiers, session credentials, CSRF tokens,
  recovery keys, or other authentication material;
- TOTP provisioning values, MFA secrets, MFA codes, or factor data;
- raw requests, raw responses, encrypted payloads, or arbitrary user-provided
  data;
- destination credentials, provider credentials, or other protected
  connection material; and
- secret values, unbounded identifiers, or source text retained solely for
  possible later redaction.

The implemented producer accepts a typed `AuditActor`, one closed `AuditEvent`,
an existing bounded workflow `CorrelationId`, and a Server-generated
`EventTime`. Human principals, Responsible Owners, and all account, Group, and
Log Module configuration event fields directly consume their typed Audit
Reference projections from the Application Database contract. Automation,
backup, component, grant, policy, Operation, and Service Connection references
accept only a bounded lowercase identifier grammar and reject raw 32-hex and
UUID-shaped database identifiers. These inputs accept neither credential-bearing
source types nor raw request values.

Terminal construction accepts an exhaustive `AuditOutcomeDetail` whose variant
must match the retained `AuditEvent`. Result-only variants use a closed
succeeded, denied, or failed outcome. State-mutating variants carry a committed
typed fact only on success: account status is active or disabled; MFA
requirement is required or optional; component and MFA Module state is enabled
or disabled; MFA reset requires re-enrollment; and the MFA Module change carries
an affected count as `u64`. Denied and failed variants have no fact payload, so
they cannot claim committed state. A mismatched detail or an impossible
successful fact is rejected with the same payload-free `InvalidOutcome`.
`LogResult` is derived from this detail rather than accepted separately.
Completion detail states the authoritative outcome and typed fact. Correction
detail begins with `corrected outcome:` and states the corrected typed fact.
No terminal input accepts a reason, previous value, provider response, raw
string, or untyped count.

`prepare_attempt` returns a `PreparedAuditAttempt` with a result-less Attempt.
Its `deliver` operation consumes the prepared value, borrows a
`ConfiguredLogDestination`, and returns a non-forgeable
`AuditAttemptReference` only after durable acknowledgement. Delivery failure
returns the shared `LogDeliveryError` unchanged and no Attempt reference.
`prepare_completion` and `prepare_correction` require that reference, derive
the original correlation identifier from it, bind the exhaustive typed outcome
detail, and mint a fresh record identifier. Terminal preparation does not accept
a second correlation identifier or a caller-selected record identifier.
`PreparedAuditTerminal::deliver(&self)` deliberately permits exact idempotent
re-delivery of the same immutable record identifier and content. It performs one
synchronous delivery per call; the producer never loops, schedules, queues,
replaces, or independently retries the record. Before the owning mutation
transaction commits, `PreparedAuditTerminal::recovery_obligation` captures the
same complete terminal record and the exact Audit destination binding as one
bounded, versioned Log-owned projection. Server Audit semantically validates
that projection, converts it and the binding's separate identity and version
into the Application Database contract's private-field opaque validated-write
wrapper, and passes no Log-owned type across the database boundary. The
projection includes the original record identifier, event time, phase, result,
Attempt link, correlation identifier, complete pre-redacted body, and
destination configuration identity and binding version. It contains no
delivery failure, setting, path, or credential. Only Server logging authority
can create a binding, and only Server Audit can perform this semantic export,
so ordinary callers cannot choose record fields or invent a destination
identity.

The producer stores no destination, catalog, queue, or retry schedule. The
Application Database recovery contract defines the opaque obligation as live
operational data outside `ApplicationState` and every backup; backend storage
does not depend on the Log contract or logging authority and does not parse or
materialize an Audit field. On import, Server Audit obtains the opaque bytes
through database persistence authority, revalidates every field through the
shared complete-record constructors, requires the separately stored obligation
identity to equal the embedded record identifier, and requires the separately
stored binding identity and version to equal the validated embedded binding. A
malformed, oversized, unsupported, identity-mismatched, binding-mismatched, or
arbitrary secret-bearing document cannot become a replayable record merely
because it was persisted. Runtime orchestration maps such an import failure to
its recovery-required state before destination access.

Server Audit also binds a constrained supersession disposition to that exact
imported obligation. The resulting trusted transaction value retains the exact
validated obligation identity and opaque projection rather than reconstructing
it from an identifier. The input boundary requires separate Server-authority
proofs for fresh exact-session password reauthentication with TOTP when
enrolled and for explicit confirmation of the exact original and replacement.
It additionally requires a replacement binding-and-destination pair that has
passed Audit preflight. No proof accepts a boolean credential, confirmation, or
preflight flag. The producer validates the fixed Log-owned disposition, then
converts its opaque bytes, exact original and replacement bindings, validated
original, and replacement terminal into the Application Database contract's
private-field supersession wrapper. The database receives no Log-owned
disposition and cannot mint one. Server Audit does not expose record fields,
verify credentials, present confirmation, retain a destination, or execute the
configuration change.

The account-create and password-reset workflows may disclose a generated
temporary password only in their originating successful response. Audit records
record issuance or reset acceptance and safe outcomes only; they must never
contain the password, verifier, response or delivery content, or whether a
human viewed or handled the response. The [Security Model](../../security-model.md#administrator-initiated-password-reset)
and [Authentication Design](../authentication/authentication-design.md#account-credential-issuance-writers)
own the disclosure and credential lifecycle policy.

Password reset and MFA reset are independent actions. Their behavior and
protected-data requirements are authoritative in the [Authentication Design](../authentication/authentication-design.md)
and [Security Model](../../security-model.md#multifactor-authentication-security-profile);
this document does not restate their session or enrollment semantics.

The owning **[Administration Plane](../../glossary.md#applications-and-interfaces)**
workflow must sequence construction and delivery as follows. The producer does
not authorize, mutate state, map client errors, construct System Logs, or own
post-commit obligations:

1. Authenticate and authorize the principal.
2. Validate the requested mutation and derive only safe intent, target, and
  action facts.
3. Ask the producer to construct and bounds-validate a pre-commit `attempt`
  record. Its content describes intent and safe target/action context only;
  it contains no final state, affected count, or other outcome-derived detail.
  It has no `result`; durable delivery acknowledgement is a separate Log Module
  contract and is not Audit record content. The complete Attempt is the only
  source of the typed `AttemptRecordId` capability used by its terminal records.
4. Deliver the attempt synchronously and wait for the Log Module's durable
  acknowledgement.
5. If construction or delivery fails, prevent the mutation and return the
  stable redacted error `Audit Log unavailable; operation rejected.` The
  workflow does not retry, enqueue, or create a substitute record. The normal
  Server process remains alive. The owning workflow records the corresponding
  System Log classification and timing under the [Log Module taxonomy](../../log-modules/log-module-design.md#event-classification-taxonomy).
6. Prepare every bounded terminal the transaction may select. The TOTP
  enablement workflow prepares one success terminal containing only desired
  state and the previewed affected-Human-User count, plus one payload-free
  denied terminal for a stale preview. The Log Module configuration workflow
  prepares one success terminal and one payload-free stale terminal after all
  resultant destinations pass preflight. The account status workflow prepares
  one success terminal containing only the resulting active or disabled status
  and one payload-free denied terminal for either a stale target or final
  issuer denial. Each directly identifies the
  acknowledged Attempt. No final state or affected count appears in the
  Attempt or denied terminal.
7. Begin the serialized application-state transaction, establish the
  authoritative outcome, and select exactly one prevalidated terminal. Capture
  each candidate with the retained current Audit destination binding before the
  transaction; atomically persist only the selected immutable obligation with
  the applied mutation or conflict result and commit both, or commit neither.
  Then invoke the bounded active-then-late drain for the exact retained record.
8. Exact destination acknowledgement authorizes acknowledgement of the oldest
  database obligation. Server Audit converts that successful Log-owned
  acknowledgement into the Application Database contract's opaque proof
  containing only the exact identity and binding; the database never receives
  destination authority. If delivery or database acknowledgement cannot
  finish, leave the obligation pending. This is a post-commit failure and must
  not map to the pre-commit `Audit Log unavailable; operation rejected.` result
  or claim that the mutation was rejected. The public route returns the
  ordinary safe committed business result without exposing whether terminal
  delivery is pending or inviting an automatic retry. Audit recovery remains
  internal. The runtime recovery path must replay obligations oldest first and
  refuse a changed current destination identity or binding version before
  delivery. It must resolve the binding and destination together through the
  trusted structural pair; binding equality alone does not prove an unrelated
  handle's identity. The contract is separate from Init and Restore lifecycle
  obligations. When the workflow asks for correction evidence, the producer
  constructs the bounded correlated `correction` with a direct link to the same
  Attempt; it does not decide when one is required.
9. An ordinary Audit destination change retains the old identity, version, and
  resolved handle while an obligation references it. If repair later proves
  that exact destination permanently unavailable, the Administration Plane may
  supersede only the exact oldest valid active obligation after fresh
  exact-session password reauthentication, fresh TOTP when enrolled, explicit
  confirmation, and replacement Audit preflight. It emits a distinct
  `dependency.audit-terminal.superseded` Attempt and terminal, then atomically
  commits the replacement assignment, append-only disposition, and new terminal
  recovery obligation. The original remains immutable and pending for exact
  late delivery; this action is neither a Correction nor delivery proof.

The same correlation identifier relates the attempt, completion or correction,
API response, and any related System Log for a request workflow, while each
terminal record's `attempt_record_id` directly identifies its precise Attempt.
Taken together, this correlated record set fulfills the
[Technical Specification's result accountability requirement](../../spec.md#logging-and-accountability),
and its before-and-after sequence fulfills the
[Operation Processing Contract](../../spec.md#operation-processing-contract).
The result-less Attempt is never sole accountability evidence: an acknowledged
Attempt proves only that intent was durably accepted, and the linked completion
or correction supplies the authoritative result. Attempt, completion, and
correction records each use a fresh record identifier.
The producer does not own the `dependency.audit-log-unavailable` System Log
record; its classification, safe context, and timing belong to the owning
workflow and [Server Observability boundary](../observability/audit-log-unavailability-record-design.md).
The Audit catalog separately names
`authorization.group-grant.removal-denied`; neither catalog entry causes the
producer to emit a System Log or orchestrate a mutation.

Public Group, member, direct-grant, and compiled-catalog reads produce no Audit
record. An exact membership or direct-grant no-op is rejected before Attempt
construction and likewise produces no Audit record. Public member and grant
changes delegate to the existing Group mutation producer without introducing a
transport-specific event: a changed membership uses
`authorization.group-membership.changed`, a changed direct grant uses
`authorization.group-grant.changed`, and an effective-last-Administrator
refusal uses `authorization.group-grant.removal-denied`. The public Group and
Account identifiers, TOTP code, opaque ticket, response projection, cursor,
and catalog payload never enter these records; the producer continues to use
only stable internal Audit references and canonical grant references.

## Operational Terminal Recovery

The `weavelit-server` operational composer owns one process-local recovery
coordinator for the selected **[Application Database](../../glossary.md#applications-and-interfaces)**.
It invokes a bounded drain once during activation and exposes the same internal
drain immediately before each consequential mutation. It runs no
background loop, timer, client route, configuration action, or independent
retry schedule. Activation recovery does not prevent the Server from exposing
read and authentication functions; a consequential writer must inspect
the active-sequence result before mutation.

Every drain obtains one process-local permit and loads the exact current Audit
configuration generation from the Application Database generation store. It
requires enabled state, Audit Log membership, a compiled-in module with the
Audit capability, and settings accepted by that module before recovery listing
or module factory access. Missing, disabled, malformed, non-Audit,
unknown-module, unsupported-capability, or undeclared-setting state fails with
one payload-free recovery-required result. Audit destination resolution does
not load or fall back to mutable `ApplicationState`.

For each retained active or late-delivery obligation, the runtime derives the
exact generation key from that obligation's separately stored binding identity
and version. It uses the loaded current generation only when that key matches
exactly; otherwise it performs one exact historical generation read. A missing,
corrupt, or mismatched required generation fails before the Log Module factory
or destination is accessed. Trusted Server code derives the binding and
configured destination together from that same snapshot and constructs one
`ResolvedAuditDestination`; it does not try another generation, the mutable
current assignment, or an independently supplied destination. The initial
committed generation uses version `1`, and later current versions do not change
the retained version named by an older obligation.

The coordinator drains active obligations first and late-delivery obligations
second. Each sequence is independently listed oldest first with at most the
Application Database contract's fixed batch maximum. A full batch returns
`Pending` so another activation or pre-consequential invocation may continue;
it does not loop through an unbounded backlog. Failure in one sequence does not
prevent the bounded attempt for the other sequence.

The database operation lane is held only while listing opaque rows or applying
one acknowledgement proof. After listing, Server Audit imports and validates
the opaque projection outside that lane, verifies its separately stored record
identity and binding, and delivers through the trusted resolved pair. Only an
exact durable destination acknowledgement becomes the database proof used to
acknowledge the oldest eligible obligation. Import, binding, delivery, or
acknowledgement failure leaves the obligation pending. Concurrent drains are
serialized across resolution, both sequences, delivery, and acknowledgement,
so two gates cannot deliver one listed obligation concurrently.

Each list, import, assignment-resolution, delivery, or acknowledgement failure
attempts one safe `dependency.audit-log-unavailable` System record through the
independently opened System Log destination. Reporting contains only the
validated destination module and the fixed `internal.log-policy.changed`
classification; raw database, projection, destination, setting, record, or
request content is never rendered. Reporting failure is absorbed. A healthy
empty activation emits no unavailable event, and terminal recovery has no
client error mapping.

## Administrative Event Taxonomy

**[Audit Logs](../../glossary.md#applications-and-interfaces)** use lowercase
dotted typed identifiers selected by the Audit
producer. The deprecated prose labels `admin password-reset` and `admin
MFA-reset` are not identifiers. A generic `action` value may distinguish an
administrative activity when the classification alone does not express that
context.

| Classification | Administrative coverage | Safe `action`, `target`, and `detail` content | Forbidden source values |
| --- | --- | --- | --- |
| `authentication.user.created` | Account creation | create; stable account reference; result | Passwords, verifiers, invitation or recovery material, response or delivery content, viewed state |
| `authentication.user.disabled` | Account disablement | disable; stable account reference; resulting status | Credentials, sessions, or request payloads |
| `authentication.password-reset.started` | Administrator-initiated password reset | reset-password; target account; issuance or reset accepted or rejected | Temporary password, verifier, response or delivery content, viewed state, tokens |
| `authentication.mfa.reset` | MFA enrollment reset | reset-mfa; target account; re-enrollment required | Provisioning value, secret, code, factor data |
| `authentication.mfa-requirement.changed` | MFA requirement changes | change-mfa-requirement; target account; new safe state | MFA secrets or authentication codes |
| `authentication.mfa-module-enablement.changed` | MFA Module enablement or disablement, including dependent-session termination | change-mfa-module; module identifier; enabled state and affected-count summary | Factor data, session identifiers, or arbitrary account data |
| `authentication.session.revoked` | Explicit session revocation when independently auditable | revoke-session; stable account or session reference; safe reason | Session or CSRF credentials |
| `authorization.group.created` | Group creation | create-group; stable group reference; result | Arbitrary request fields or secrets |
| `authorization.group.updated` | Group name or description update | update-group; stable group reference; result | Names, descriptions, public or state identifiers |
| `authorization.group.deleted` | Empty Group deletion | delete-group; stable group reference; result | Names, public or state identifiers, memberships, grants, counts, TOTP evidence or tickets |
| `authorization.group-membership.changed` | Group membership changes | change-membership; stable group and account references; result | Unbounded submitted member data |
| `authorization.group-grant.changed` | Client, Service, Operation, or Server Administration Permission grant changes | change-grant; stable group and grant references; result | Credentials or unbounded policy payloads |
| `authorization.group-grant.removal-denied` | Rejected membership or direct-grant removal that would remove the last effective Server Administration Permission | remove-membership or remove-grant; stable group and account or canonical grant references; last-administrator rejection | Internal authorization detail beyond the safe reason |
| `authorization.automation-scope.changed` | Automation Identity Operation-scope changes | change-automation-scope; stable identity and Operation references; result | Automation credentials or secret configuration |
| `dependency.audit-terminal.superseded` | Constrained supersession of one permanently undeliverable terminal obligation | supersede-terminal-delivery; bounded terminal reference; degraded completeness | Destination errors, settings, credentials, reauthentication or TOTP evidence, confirmation content, raw identifiers |
| `dependency.log-module-configuration.changed` | Log Module configuration changes | change-log-module-configuration; stable configuration/module reference; result | Destination credentials, paths, raw settings, or payloads |
| `dependency.service-connection.changed` | Service Connection configuration changes | change-service-connection; stable connection reference; result | Provider credentials, tokens, keys, or raw configuration |
| `internal.server-configuration.changed` | Other Server, Client Module, Service Module, or Operation enablement changes | change-server-configuration; stable component reference; safe state | Secrets, raw configuration, or arbitrary submitted values |
| `internal.user-status.changed` | Account status changes not represented by `authentication.user.disabled` | change-user-status; stable account reference; safe status | Credentials, session values, or raw account data |
| `internal.log-policy.changed` | Log policy changes owned by the Server | change-log-policy; stable policy reference; safe result | Retention secrets, destination credentials, or raw settings |
| `provider.operation.started` | Consequential supported Operation start | operation-start; typed Operation and safe target; accepted | Provider request/response bodies or credentials |
| `provider.operation.completed` | Consequential supported Operation completion | operation-complete; typed Operation and safe target; result summary | Provider payloads, responses, or returned sensitive data |

Both provider rows come from one `ProviderOperation` producer event. Its
Attempt is classified as `provider.operation.started` with `operation-start`;
its linked completion or correction is classified as
`provider.operation.completed` with `operation-complete`. The shared typed
classification catalog remains closed, and terminal recovery rejects unknown
future values.

The producer may also use the remaining approved catalog entries
`lifecycle.backup.created`, `authentication.password.changed`, and
`authentication.mfa.enrolled` where their owning Server contract makes the
action consequential. It must not invent a new identifier to avoid a taxonomy
decision. The canonical full catalog remains in [Log Module Design](../../log-modules/log-module-design.md#event-classification-taxonomy).

The implemented producer owns typed, bounded attempt, completion, and
correction construction, including direct Attempt linkage, and synchronous
delivery to a supplied configured Audit destination. It also owns trusted
export and import of the immutable normal-operation terminal recovery
projection. Administration Plane workflow work decides when a correction is
required and owns assignment resolution, mutation sequencing, client-error
mapping, System Log emission, credential verification, confirmation
presentation, configuration generations, and runtime drain policy. The
producer owns only the typed supersession event and exact imported-obligation
disposition boundary; it does not select or execute those policies.

## Retention And Validation Implications

**[Audit Logs](../../glossary.md#applications-and-interfaces)** must not be
automatically purged. Destination retention, storage
durability, backup, recovery, and compatibility remain owned by the Log Module
design and its destination; this document does not promise indefinite survival
or add a Server-wide retention mechanism.

Focused validation for the producer, the account credential writers, the TOTP
enablement workflow, the Log Module configuration workflow, and future
administration contracts
must prove:

- every field rejects empty or over-bound UTF-8 input, including the 8 KiB
  aggregate limit, without truncation, hashing, or source retention;
- human and automation principal shapes enforce the Responsible Owner rule;
- only an Attempt can mint an opaque link capability; every new completion and
  correction requires that link and matching correlation; and terminal links
  cannot target an absent, later, non-Attempt, or differently correlated record;
- every taxonomy value is the exact canonical dotted identifier, and password
  reset and MFA reset remain separate;
- forbidden values cannot enter action, target, or detail through fixed,
  allowlisted, or structured summaries;
- account, Group, and Log Module configuration principal or target values
  contain only the persisted typed `ar-...` projection even when the source
  entity has a broad Unicode name or a distinct internal state identifier;
- every event accepts only its matching exhaustive outcome-detail variant,
  successful state details require their typed committed fact, denied and
  failed details carry none, and completion and correction summaries derive the
  expected result and fact;
- one provider event maps its Attempt and terminal phases to the paired started
  and completed classifications and actions;
- construction and destination failure prevent a consequential state commit,
  return the stable redacted error, and leave the process alive while the
  owning workflow records `dependency.audit-log-unavailable`; and
- the attempt is acknowledged before the corresponding mutation commit, the
  terminal record follows the authoritative transaction outcome, the exact
  terminal projection commits atomically with that outcome, and a post-commit
  delivery failure remains pending without reusing an Init or Restore lifecycle
  obligation or returning the pre-commit rejection;
- projection import revalidates every immutable field and matching identity,
  changed current bindings prevent destination calls, replay is oldest first,
  and database acknowledgement follows only exact destination acknowledgement;
- ordinary binding transitions retain the exact prior identity and version;
  supersession accepts only matching authority, confirmation, and preflight
  evidence for the exact oldest valid active obligation; a stored original with
  the same identity but different projection bytes or retained binding is
  rejected before mutation; its fixed disposition remains secret-free and
  degraded; and exact late delivery through the old binding remains possible
  after the replacement becomes active;
- exact repeated delivery of one immutable prepared terminal record is
  idempotent at the SQLite destination while the producer performs no delivery
  loop, schedule, replacement, or recovery decision; and
- no retrieval or export behavior is implied by the producer contract.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Server API Contract](../api/api-contract-design.md)
- [Authentication Design](../authentication/authentication-design.md)
- [Authorization Design](../authorization/authorization-design.md)
- [Audit Log Unavailability System Log Record](../observability/audit-log-unavailability-record-design.md)
- [Glossary](../../glossary.md)
- [Temporary Password Disclosure Decision](../authentication/temporary-password-disclosure-decision.md)
- [Testing and Validation Policy](../../testing.md)
- [Audit Terminal Binding Retention And Supersession Decision](../../log-modules/audit-terminal-binding-retention-decision.md)
