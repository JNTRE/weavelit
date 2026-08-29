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

The browser MUST invoke the route only according to the shared
[API Contract Design](../../server/api/api-contract-design.md#application-database-backup-download):

- send the session cookie and session `X-Weavelit-CSRF` header according to the
  general API contract;
- send exactly same-origin `Origin` and `Host` values;
- send no request body and no `Content-Type` header; and
- omit `Accept` or send exactly `Accept: application/octet-stream`.

The Client Module and browser do not treat a visible route or a client-side
check as authorization. The Server remains the sole authentication,
validation, and authorization authority.

## Download And Failure Handling

On a fully received `200 OK`, the browser handles the raw binary response as
the API contract's attachment named `weavelit-backup.wlitbackup`. It honors the
Server-defined `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`
response profile. The browser does not receive or construct an artifact URL,
identifier, storage location, resume mechanism, retry mechanism,
reconciliation mechanism, or later-retrieval capability.

The browser MUST NOT interpret a listener loss, timeout, unreadable response,
or incomplete binary attachment as success. It MUST NOT automatically retry,
resume, reconcile, or retrieve an artifact after such an indeterminate result.
A later explicit request is a distinct backup creation under the API contract.

Every non-success response remains the API contract's typed JSON error response
with a Server-generated correlation identifier. The browser may use that
identifier for support correlation, but it MUST NOT infer or expose Server
internals, artifact details, authorization details, or another sensitive cause
beyond the contract's stable error result.

## Implementation And Validation Obligations

Implementation of this declaration MUST preserve the shared route's exact
request, successful binary attachment, typed-error, correlation, and
indeterminate-outcome semantics. Focused Client Module and API contract tests
MUST cover the accepted request profile, rejected request variants, Server-side
authorization independence, the closed binary response headers, typed redacted
errors, and the rule that only a fully received `200 OK` proves success, as
required by the [Testing and Validation Policy](../../testing.md).

## Related Documents

- [Web UI Client Module Agent Guide](AGENTS.md)
- [API Contract Design](../../server/api/api-contract-design.md)
- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Testing and Validation Policy](../../testing.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
