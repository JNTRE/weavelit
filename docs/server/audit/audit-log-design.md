# Server Audit Log Design

This document owns the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
producer contract for **[Audit Logs](../../glossary.md#applications-and-interfaces)**.
It defines how a consequential authenticated action becomes one bounded,
pre-redacted accountability record before delivery. The shared complete-record
envelope, destination acknowledgement, classification catalog implementation,
and destination storage remain defined by the [Log Module Design](../../log-modules/log-module-design.md).
This document does not define Audit Log retrieval, query, export, client
presentation, destination-specific storage, or destination redaction.

## Ownership And Invariants

The Server owns authorization, Audit producer construction, pre-redaction, and
the decision to commit a consequential mutation. The `weavelit-server-audit`
component constructs the Audit body and supplies it to the
`weavelit-server-log` contract. A Log Module receives only the complete
immutable record; it must not redact, enrich, reinterpret, or read Application
Database state.

Every consequential authenticated application action must be attributable to
the authenticated principal and must produce an Audit Log record. Init and
Restore are pre-operational lifecycle actions and produce System Logs, not
Audit Logs. Operational diagnosis remains in the Server Observability boundary.

## Bounded Record Contract

The Server generates the opaque `record_id`, UTC Unix-millisecond
`event_time`, and `correlation_id` in trusted Server context. The producer
constructs the following pre-redacted Audit body and complete record:

| Field | Required contract |
| --- | --- |
| `record_id` | Server-generated, opaque, nonzero 16-byte identifier. A caller, producer input, or destination must not choose it. |
| `event_time` | Server-generated UTC Unix time in milliseconds. |
| `result` | Exactly `success` or `failure`. |
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
with different content.

## Construction, Redaction, And Delivery

The producer receives only the validated facts needed to describe the action.
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

An Administrator-initiated password reset must not reveal the temporary
password to the Administrator. This design records only the reset action and
its safe result; it does not select or document a delivery channel for the
temporary password.

Password reset and MFA reset are independent actions. A password reset record
must not imply an MFA reset, and an MFA reset record must not imply a password
reset. Disabling an MFA Module terminates sessions that depend on that method,
but preserves existing enrollments, MFA requirement state, Audit history, and
other persisted data. This session effect is recorded as the safe enablement
change result; it is not a data purge.

The route or owning Server workflow delivers the complete record synchronously
through the configured Audit Log assignment. For an Administration Plane
mutation, the sequence is:

1. Authenticate and authorize the principal.
2. Validate the requested mutation and derive safe record fields.
3. Construct and bounds-validate the complete Audit record.
4. Deliver it and wait for the Log Module's durable acknowledgement.
5. Commit the application-state mutation only after successful delivery.

Construction or delivery failure rejects or rolls back the mutation. The
caller receives the stable redacted error `Audit Log unavailable; operation
rejected.` A failed attempt does not create a substitute record and does not
leave the mutation committed. The normal Server process remains alive after a
destination becomes unavailable.

If the application-state commit fails after the Audit record is durably
acknowledged, the workflow emits a failure Audit correction using the same
correlation identifier and safe target and action context. It also records a
best-effort System Log, then returns a stable redacted failure without
reporting success. The acknowledged success record must not be treated as
proof that the mutation committed.

The Server records destination failures as the System Log classification
`dependency.audit-log-unavailable`, with only safe destination and operation
context. It does not add an outbox, retry queue, background delivery path, or
destination-side enrichment. Non-consequential operations may absorb an
Audit-delivery failure only where the owning route permits it; the failure must
still be visible through the System Log.

## Administrative Event Taxonomy

Classifications are lowercase dotted typed identifiers selected by the Audit
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

The producer may also use the remaining approved catalog entries
`lifecycle.backup.created`, `authentication.password.changed`, and
`authentication.mfa.enrolled` where their owning Server contract makes the
action consequential. It must not invent a new identifier to avoid a taxonomy
decision. The canonical full catalog remains in [Log Module Design](../../log-modules/log-module-design.md#event-classification-taxonomy).

## Retention And Validation Implications

Audit destination retention is indefinite in Milestone 1: no automatic Audit
Log purge occurs. Destination retention, backup, recovery, and storage
compatibility remain owned by the Log Module design and its destination; this
document does not add a Server-wide retention mechanism.

Focused validation for the producer and its future administration contracts
must prove:

- every field rejects empty or over-bound UTF-8 input, including the 8 KiB
  aggregate limit, without truncation, hashing, or source retention;
- human and automation principal shapes enforce the Responsible Owner rule;
- every taxonomy value is the exact canonical dotted identifier, and password
  reset and MFA reset remain separate;
- forbidden values cannot enter action, target, or detail through fixed,
  allowlisted, or structured summaries;
- construction and destination failure prevent a consequential state commit,
  return the stable redacted error, emit
  `dependency.audit-log-unavailable`, and leave the process alive; and
- successful delivery precedes the corresponding mutation commit, and a
  post-acknowledgement commit failure emits the failure Audit correction and
  best-effort System Log without reporting success; and
- no retrieval or export behavior is implied by the producer contract.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Log Module Design](../../log-modules/log-module-design.md)
- [Authentication Design](../authentication/authentication-design.md)
- [Authorization Design](../authorization/authorization-design.md)
- [Glossary](../../glossary.md)
- [Testing and Validation Policy](../../testing.md)
