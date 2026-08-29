# Web UI Administration Backup Capability

This document declares the normal-operation Administration Plane backup-download
capability of the Web UI Client Module and its browser responsibilities under
the settled Server API contract. [API Contract Design](../../server/api/api-contract-design.md#application-database-backup-download) and
[Security Model](../../security-model.md#secret-disclosure-cache-control) remain authoritative for the route and response security profile.

## Represented Areas

| Type | Link |
| --- | --- |
| Client Module | [Web UI Client Module](AGENTS.md) |
| API contract | [Application Database Backup Download](../../server/api/api-contract-design.md#application-database-backup-download) |
| Security profile | [Secret Disclosure Cache Control](../../security-model.md#secret-disclosure-cache-control) |
| Validation policy | [Testing and Validation Policy](../../testing.md) |

## Scope And Ownership

The **[Web UI](../../glossary.md#applications-and-interfaces)**
**[Client Module](../../glossary.md#applications-and-interfaces)** declares one
normal-operation **[Administration Plane](../../glossary.md#applications-and-interfaces)**
capability. It is not a **[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)**
capability and provides no backup capability while the Server is pre-operational.

This document does not define visible Web UI controls or workflows; the
[Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
owns those concerns. It also does not define backup creation, snapshotting,
writing, retention, encrypted format, artifact resource lifecycle, audit timing,
or Restore behavior. Those Server-owned concerns are outside this capability
declaration.

## Capability Declaration

The Client Module declares exactly this route through the shared API contract:

```http
PUT /api/v1/administration/backups/create
```

The capability makes the route available as an Administration Plane function;
it does not decide access. The Server independently authorizes the request from
the authenticated session and live Server state. An
**[Administrator](../../glossary.md#identities-and-access)** session requires
access to this Client Module and the effective
**[Server Administration Permission](../../glossary.md#identities-and-access)**.
The browser supplies no identity, Client Module, grant, or permission claim.

## Browser Request Requirements

For each explicit backup creation, the browser MUST first invoke the existing
Administration **[Multifactor Authentication (MFA)](../../glossary.md#identities-and-access)**
step-up route with `PUT /api/v1/administration/step-up/totp`, the session
cookie and session `X-Weavelit-CSRF` header, `credentials: same-origin`, JSON
media types, and exactly this strict JSON body:

```json
{"family":"backup_create","code":"123456"}
```

A successful step-up response carries the opaque ticket only at
`result.totp_step_up_ticket` in the typed JSON envelope. The browser then
MUST send that ticket only in one immediate backup request, with the session
cookie and session `X-Weavelit-CSRF` header, `credentials: same-origin`, and
the browser-generated same-origin `Origin` and `Host` values. That request
MUST carry exactly `Content-Type: application/json`, omit `Accept` or carry
exactly `Accept: application/octet-stream`, and have exactly this strict JSON
body:

```json
{"backup_create_step_up_ticket":"<43-character canonical Base64url>"}
```

The ticket is transient application memory for that immediate action only. It
MUST NOT enter a URL, cookie, browser storage, log, telemetry, DOM, rendered
page, retry queue, retained artifact, or later request. The browser MUST
discard its retained ticket immediately after initiating the backup request,
regardless of the response, and MUST NOT reuse it after that attempt. A later
explicit backup creation starts a new `backup_create` step-up and obtains a new
ticket. This browser one-attempt rule does not alter the Server contract under
which a matching ticket remains reusable until its five-minute expiry.

The backup request accepts no password, TOTP code, identity, Client Module,
grant, permission claim, caller-controlled step-up flag, `Idempotency-Key`, or
other request field. The Client Module and browser do not treat a visible route
or a client-side check as authorization. The Server remains the sole
authentication, validation, and authorization authority.

Malformed step-up issuance or backup-request transport is the typed
`bad_request` result. A valid-shaped backup ticket with no retained proof, an
expired proof, or a family, session, actor, Client Module, or accepted-factor
mismatch is the generic typed `authorization_denied` result. Neither response
exposes ticket, binding, factor, family, expiry, or other sensitive detail.

## Download And Failure Handling

On a `200 OK`, the browser MUST treat the download as successful only when it
can completely read the raw binary response and its body contains exactly the
advertised `Content-Length` bytes. It then handles the binary response as the
API contract's attachment named `weavelit-backup.wlitbackup`. It honors the
Server-defined `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`
response profile. The browser does not receive or construct an artifact URL,
identifier, storage location, resume mechanism, retry mechanism,
reconciliation mechanism, or later-retrieval capability.

The browser MUST treat a listener loss, timeout, short, unreadable, malformed,
or length-mismatched binary response as incomplete, not successful. It MUST
NOT automatically retry, resume, reconcile, or retrieve an artifact after an
indeterminate or incomplete result. A later explicit request is a distinct
backup creation under the API contract.

Every non-success response remains the API contract's typed JSON error response
with a Server-generated correlation identifier. The browser may use that
identifier for support correlation, but it MUST NOT infer or expose Server
internals, artifact details, authorization details, or another sensitive cause
beyond the contract's stable error result.

## Implementation And Validation Obligations

Implementation of this declaration MUST preserve the shared route's exact
step-up issuance and backup request, successful binary attachment, typed-error,
correlation, and indeterminate-outcome semantics. Focused Client Module, API
contract, and browser binary-download handling tests MUST cover
family-specific `backup_create` issuance; its exact request schema and JSON
content type; the exact backup request schema, JSON content type, and allowed
`Accept` forms; session credentials and CSRF handling; and the absence of
ticket persistence or exposure. They MUST cover absent, missing, malformed,
mismatched, and expired tickets, including `bad_request` for malformed
transport and generic `authorization_denied` for valid-shaped proof rejection.

Tests MUST cover Server-side authorization independence, the closed binary
response headers, and header/body completion: only a completely readable
`200 OK` body with exactly its advertised `Content-Length` proves success. They
MUST also cover short, unreadable, malformed, and length-mismatched responses
as incomplete rather than successful, including the absence of automatic retry,
resume, reconciliation, or retrieval; disposal of the ticket after one backup
attempt; and the requirement for a fresh explicit request and step-up. Tests
MUST cover typed redacted errors, as required by the
[Testing and Validation Policy](../../testing.md).

## Related Documents

- [Web UI Client Module Agent Guide](AGENTS.md)
- [API Contract Design](../../server/api/api-contract-design.md)
- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Testing and Validation Policy](../../testing.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
