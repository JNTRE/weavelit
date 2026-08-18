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

`weavelit-server-audit` implements this producer boundary. The
**[Administration Plane](../../glossary.md#applications-and-interfaces)**
workflows that consume it remain future work.

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
identifier while retaining distinct record identifiers. Audit construction does
not accept a standalone Application Database `StateIdentifier`, name, or raw
account or Group string. Account and Group fields consume the database
contract's typed persisted Audit projections and render only their
`audit_reference()` value as `account:ar-...` or `group:ar-...`; they never read
or serialize the projection's state identifier. The producer renders every
other safe target from its closed typed input. The shared envelope remains the
schema authority for the typed phase, terminal result, and Attempt link
invariant.

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
`EventTime`. Human principals, Responsible Owners, and all account and Group
event fields directly consume `AccountAuditReference` or `GroupAuditReference`
from the Application Database contract. Automation, backup, component, grant,
configuration, module, policy, Operation, and Service Connection references
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
replaces, or independently retries the record. The producer stores no
destination, catalog, authority, queue, or post-commit obligation. A future
Administration Plane workflow decides whether and how a retained terminal
record participates in durable post-commit recovery.

An Administrator-initiated password reset must not reveal the temporary
password to the Administrator. This design records only the reset action and
its safe result; it does not select or document a delivery channel for the
temporary password.

Password reset and MFA reset are independent actions. Their behavior and
protected-data requirements are authoritative in the [Authentication Design](../authentication/authentication-design.md)
and [Security Model](../../security-model.md#multifactor-authentication-security-profile);
this document does not restate their session or enrollment semantics.

The owning **[Administration Plane](../../glossary.md#applications-and-interfaces)**
workflow sequences construction and delivery as follows. The producer does
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
6. Apply the atomic application-state mutation.
7. After the mutation outcome is authoritative, ask the producer to construct
  and synchronously deliver a correlated `completion` record. Its `result`
  is derived from the matching typed detail, whose safe fact represents the
  committed success or whose payload-free denied or failed variant represents
  the known non-commit outcome. Its `attempt_record_id` directly identifies the
  acknowledged Attempt. Final state and affected count may appear only in a
  matching typed completion detail or a later matching correction detail.
8. If completion delivery cannot finish after commit, the owning workflow
  triggers and durably manages the future normal-operation recovery contract.
  That contract is separate from the Init and Restore lifecycle obligations and
  remains future Administration Plane workflow design. When the workflow asks
  for correction evidence, the producer constructs the bounded correlated
  `correction` with a direct link to the same Attempt; it does not decide when
  one is required or make it durable.

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

## Administrative Event Taxonomy

**[Audit Logs](../../glossary.md#applications-and-interfaces)** use lowercase
dotted typed identifiers selected by the Audit
producer. The deprecated prose labels `admin password-reset` and `admin
MFA-reset` are not identifiers. A generic `action` value may distinguish an
administrative activity when the classification alone does not express that
context.

| Classification | Administrative coverage | Safe `action`, `target`, and `detail` content | Forbidden source values |
| --- | --- | --- | --- |
| `authentication.user.created` | Account creation | create; stable account reference; result | Passwords, verifiers, invitation or recovery material |
| `authentication.user.disabled` | Account disablement | disable; stable account reference; resulting status | Credentials, sessions, or request payloads |
| `authentication.password-reset.started` | Administrator-initiated password reset | reset-password; target account; reset accepted or rejected | Temporary password, verifier, delivery contents, tokens |
| `authentication.mfa.reset` | MFA enrollment reset | reset-mfa; target account; re-enrollment required | Provisioning value, secret, code, factor data |
| `authentication.mfa-requirement.changed` | MFA requirement changes | change-mfa-requirement; target account; new safe state | MFA secrets or authentication codes |
| `authentication.mfa-module-enablement.changed` | MFA Module enablement or disablement, including dependent-session termination | change-mfa-module; module identifier; enabled state and affected-count summary | Factor data, session identifiers, or arbitrary account data |
| `authentication.session.revoked` | Explicit session revocation when independently auditable | revoke-session; stable account or session reference; safe reason | Session or CSRF credentials |
| `authorization.group.created` | Group creation | create-group; stable group reference; result | Arbitrary request fields or secrets |
| `authorization.group-membership.changed` | Group membership changes | change-membership; stable group and account references; result | Unbounded submitted member data |
| `authorization.group-grant.changed` | Client, Service, Operation, or Server Administration Permission grant changes | change-grant; stable group and grant references; result | Credentials or unbounded policy payloads |
| `authorization.group-grant.removal-denied` | Rejected attempt to remove the last Server Administration Permission | remove-grant; stable group/account references; last-grant rejection | Internal authorization detail beyond the safe reason |
| `authorization.automation-scope.changed` | Automation Identity Operation-scope changes | change-automation-scope; stable identity and Operation references; result | Automation credentials or secret configuration |
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
classification catalog and persisted Log schema remain unchanged.

The producer may also use the remaining approved catalog entries
`lifecycle.backup.created`, `authentication.password.changed`, and
`authentication.mfa.enrolled` where their owning Server contract makes the
action consequential. It must not invent a new identifier to avoid a taxonomy
decision. The canonical full catalog remains in [Log Module Design](../../log-modules/log-module-design.md#event-classification-taxonomy).

The implemented producer owns typed, bounded attempt, completion, and
correction construction, including direct Attempt linkage, and synchronous
delivery to a supplied configured Audit destination. Future Administration
Plane workflow work decides when a correction is required and owns assignment
resolution, mutation sequencing, client-error mapping, System Log emission, and
the durable normal-operation recovery contract for a completion that cannot be
delivered after commit. This producer design does not select that recovery
mechanism.

## Retention And Validation Implications

**[Audit Logs](../../glossary.md#applications-and-interfaces)** must not be
automatically purged. Destination retention, storage
durability, backup, recovery, and compatibility remain owned by the Log Module
design and its destination; this document does not promise indefinite survival
or add a Server-wide retention mechanism.

Focused validation for the producer and future administration contracts
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
- account and Group principal or target values contain only the persisted typed
  `ar-...` projection even when the source entity has a broad Unicode name or a
  distinct internal state identifier;
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
  completion follows the authoritative outcome, and a post-commit delivery
  failure remains with the owning workflow's future normal-operation recovery
  contract without reusing an Init or Restore lifecycle obligation;
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
- [Testing and Validation Policy](../../testing.md)
