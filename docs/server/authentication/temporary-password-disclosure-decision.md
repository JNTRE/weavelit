# Temporary Password Disclosure Decision

## Status

Accepted. This record preserves the approved policy decision; the [Security
Model](../../security-model.md) owns the binding security profile and the
[Authentication Design](authentication-design.md) owns its future
implementation design.

## Context

The previous policy prohibited an Administrator from receiving a
Server-generated temporary password, but it provided no usable delivery
channel for account creation or password reset. The approved operational
intent is that an authorized **[Administrator](../../glossary.md#identities-and-access)**
receives the temporary password in the originating successful response and
shares or stores it outside Weavelit as needed.

Server-side retrieval, continuation, token-retention, and later re-disclosure
alternatives were rejected because they would either retain a bearer secret or
re-disclose plaintext after the originating workflow. The decision reverses
the prior Security Model `MUST NOT` prohibition.

## Decision

The Server may disclose a generated temporary password in plaintext only once,
in the originating successful account-create or password-reset response. The
Server must not recover, re-disclose, persist, log, or expose the plaintext
through errors, diagnostics, URLs, cookies, or browser storage. A lost response
requires an explicit new reset; there is no automatic retry or later retrieval.

The Server's non-recoverability guarantee applies to the Server. It does not
guarantee that an Administrator or client cannot copy or retain the value. The
authorized Administrator is responsible for external sharing and custody.
Audit records may record issuance or reset outcome and must never contain the
password, verifier, response or delivery content, or viewed state.

This policy applies only to future account-create and password-reset
workflows. Init remains unchanged and does not use this operational disclosure
flow.

## Consequences

The authorized Administrator becomes the temporary credential's custodian and
is responsible for delivering it through an external channel. The [Security
Model](../../security-model.md#administrator-initiated-password-reset) and
[Authentication Design](authentication-design.md#account-credential-issuance-writers)
define the concrete safeguards, expiry, session, revision, and reauthentication
requirements.

If an Administrator loses an issued value or it expires, recovery requires a
new Administrator reset. A self-reset whose result is lost or expired can lock
the Administrator out; if that account is the last Administrator, the
deployment may become inaccessible through supported interfaces. This is an
accepted fail-closed risk and remains the responsibility of deployment
operators. This record does not claim that any route or workflow is currently
implemented.

## Rejected Alternatives

- Server-side retrieval after creation or reset would require retaining a
  recoverable secret or re-disclosing plaintext.
- A continuation, bearer token, or retained response record would extend the
  secret's lifetime and create another retrieval channel.
- Automatic retry or later re-disclosure would make an indeterminate or lost
  response ambiguous and would violate one-response disclosure.
- Prohibiting all disclosure leaves the approved account-management workflows
  without an operational delivery channel.

## Related Documents

- [Security Model](../../security-model.md)
- [Authentication Design](authentication-design.md)
- [Audit Log Design](../audit/audit-log-design.md)
- [API Contract Design](../api/api-contract-design.md)
- [Technical Specification](../../spec.md)
