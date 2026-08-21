# Weavelit Server API Contract

This document owns the version 1 application interface contract of the
**[Weavelit Server](../../glossary.md#applications-and-interfaces)**: how
**[Client Modules](../../glossary.md#applications-and-interfaces)** compose and
declare that interface, how routes are organized and versioned, how results and
errors are represented, and what compatibility the Server promises.

It does not own service-specific **[Operation](../../glossary.md#applications-and-interfaces)**
semantics, which belong in `../../service-modules/`. It does not own
lifecycle gating, **[Init](../../glossary.md#states-and-requests)**, or
**[Restore](../../glossary.md#states-and-requests)** workflow behavior, which
belong to the [Server Lifecycle Design](../lifecycle/lifecycle-design.md). It
does not own the session, cookie, or multifactor security profile, which belongs
to the [Server Authentication Design](../authentication/authentication-design.md).
It does not own grant evaluation, which belongs to the
[Server Authorization Design](../authorization/authorization-design.md).

## Client Module Composition

A Client Module is a Server-side crate that declares and registers an API
surface. The client application that consumes it is a separate program.

The contract itself lives in one shared crate. Per-module crates own only what
genuinely differs between clients:

- `weavelit-module-client` owns the request schemas, handlers, validation,
  results, stable error codes, plane definitions, and capability declarations.
  It is the single place the contract changes.
- `weavelit-module-client-webui` owns browser-specific presentation, the
  embedded application assets, and the capabilities the Web UI declares.
- A future Weavelit CLI Client Module owns CLI-specific presentation and its own
  declared capabilities.

Two Client Modules that declare the same function serve one implementation, so
their behavior cannot diverge. The [Technical Specification](../../spec.md)
permits this composition and requires that shared implementation never weaken
declaration.

### Capability Declaration

A Client Module declares its surface by returning a registration value whose
populated fields are the declaration. Presence is the declaration, so declared
capability cannot drift from what the Server mounts. The Server registers only
what a module returns, and an undeclared plane or capability is absent rather
than present and denied.

A Client Module declares each of the following independently:

- pre-operational capabilities, each gated to the lifecycle states in which it
  is eligible;
- a **[User Plane](../../glossary.md#applications-and-interfaces)**, an
  **[Administration Plane](../../glossary.md#applications-and-interfaces)**, or
  both; and
- the access class each declared function requires.

Declaration determines interface capability only. The Server core independently
authorizes every request.

## Route Organization

API routes are versioned under `/api/v1/`. Routes are global rather than
namespaced by Client Module, so every client invokes the same function through
the same route and observes the same behavior.

Route groups:

- `/api/v1/status` and `/api/v1/application-database` are Server lifecycle
  contracts. Their behavior is fixed by the
  [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
  and the
  [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md).
- `/api/v1/init/recovery-key` and `/api/v1/init` are the two requests of the
  Init submission protocol. Their behavior is fixed by the
  [Web UI Pre-Operational Init Surface](../../client-modules/web-ui/pre-operational-init-design.md).
  The Web UI Client Module declares this contract and the `weavelit-server`
  runtime mounts it, so Init is reachable end to end over the API, and the
  [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
  Init workflow drives it through the browser.
- `/api/v1/restore` and `/api/v1/restore/artifact` are the two requests of the
  Restore submission protocol. Their behavior is fixed by the
  [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md).
- Pre-operational lifecycle contracts are mounted only while their declaring
  capability is eligible.
- `/api/v1/lifecycle/reconciliation` is an operational route, mounted once
  normal operation begins rather than while a pre-operational capability is
  eligible. It confirms a submission-bound Init or Restore capability and is
  defined in full in [Lifecycle Reconciliation](#lifecycle-reconciliation)
  below.
- `/api/v1/auth/` carries authentication bootstrap. These routes are neither
  User Plane nor Administration Plane, because a principal does not yet exist
  when they are invoked. Seven routes are defined, all `PUT`:
  - `/api/v1/auth/login` exchanges a `client_module` identity and local
    credentials for a session, subject to the single-permit admission lane
    the [Server Authentication Design](../authentication/authentication-design.md#login-admission-and-verification-concurrency)
    owns. A verified password that requires no further step returns `200` and
    the session-issuing cookie effect. A verified password that must present a
    second factor or must enroll one instead returns `202` with a
    [continuation](../authentication/authentication-design.md#continuation-ticket)
    result and no cookie:

    ```json
    {"result":{"mfa":"mfa_required","continuation":"<opaque-server-value>"},
     "correlation_id":"<opaque-server-value>"}
    ```

    `mfa` carries `mfa_required` when an enrolled factor must verify a code, or
    `mfa_enrollment_required` when the account must enroll one first. Both
    codes are defined by the shared `weavelit-module-client` crate; only their
    values differ.
  - `/api/v1/auth/session` validates the session cookie already presented and
    reports whether it is still active, issuing no new cookie. Its successful
    result carries the existing lowercase-hexadecimal `account_id` for
    compatibility and the account's independent `public_id`, encoded as exactly
    22 unpadded Base64url characters. New account-administration targets accept
    only `public_id`; `account_id` remains limited to this additive session
    result. It also carries the boolean `password_change_required`, derived from
    the live validated session posture. A `true` result identifies a restricted
    session that can only replace its temporary password.
  - `/api/v1/auth/password/change` replaces the current restricted session's
    temporary password. It requires the session cookie, the session's
    `X-Weavelit-CSRF` value, exact same-origin and `Host` validation, and a
    strict JSON body containing only a non-empty `password` of at most 1,024
    bytes. It accepts no target identifier, current password, temporary
    password, or caller-provided session state. On success it returns `200` and
    the fresh ordinary session cookie effect. A malformed or refused request is
    payload-free and reveals neither the reason nor temporary credential state.
    A client MUST NOT automatically retry an unreadable or timed-out submission;
    it may only reconcile through `/api/v1/auth/session`.
  - `/api/v1/auth/logout` revokes the presented session and clears both
    cookies.
  - `/api/v1/auth/mfa/verify` submits a `code` against the `continuation` a
    `mfa_required` login response carried. Success returns `200` and the
    session-issuing cookie effect, exactly as a login that required no second
    factor does. Every refusal — an unknown or expired continuation, a wrong
    code, a replayed code, or a Module disabled since the continuation was
    issued — is the same `401` `authentication_failed` response login itself
    uses, and the continuation is consumed whether or not the code was
    correct.
  - `/api/v1/auth/mfa/enrollment` opens an enrollment from the `continuation` a
    `mfa_enrollment_required` login response carried. Success returns `200`
    with the one-time provisioning result:

    ```json
    {"result":{"secret":"<base32-value>",
               "provisioning_uri":"<otpauth-uri>",
               "enrollment":"<opaque-server-value>"},
     "correlation_id":"<opaque-server-value>"}
    ```

    `secret` and `provisioning_uri` are returned in this one response and are
    never retrievable again; `enrollment` is a second, separate continuation
    that confirms this exact enrollment.
  - `/api/v1/auth/mfa/enrollment/confirm` submits a `code` against the
    `enrollment` value the enrollment-opening response carried. Success returns
    `200` and the session-issuing cookie effect. Refusal is the same `401`
    `authentication_failed` response every second-factor refusal uses, and the
    enrollment ticket is consumed whether or not the code was correct.
  - `/api/v1/auth/mfa/enrollment/session` opens an enrollment for the account
    of an already-established, session-bearing request rather than a login
    continuation. It requires the session cookie, its per-session
    `X-Weavelit-CSRF` header value, and the account's current `password` in
    the request body, all re-verified through the same password check login
    uses. Success returns the same `200` provisioning result
    `/api/v1/auth/mfa/enrollment` returns; a wrong password returns the same
    `401` `authentication_failed` response a wrong login password does.
  The shared `weavelit-module-client` crate owns every stable error code and
  header precondition for these seven routes; the decisions behind them are
  owned by the
  [Server Authentication Design](../authentication/authentication-design.md).
- `/api/v1/user/` and `/api/v1/administration/` carry the two normal planes.

### Account Administration Reads

The account-read surface contains exactly two routes:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/accounts/list` | `PUT` | One bounded account page in ascending immutable username order. |
| `/api/v1/administration/accounts/view` | `PUT` | One account selected by its Account Public Identifier. |

Both routes require an ordinary validated session, exact same-origin `Origin`
and `Host`, the session's `X-Weavelit-CSRF` value, an acceptable JSON response
media type, live Web UI Client Module access, and the effective Server
Administration Permission. Authorization uses the Client Module stored in the
session and accepts no caller-supplied account, Group, grant, or permission
claim. Reads create no Audit record and change no account, session, or MFA
state.

The list body is optional. An absent body and `{}` both select the default
limit of 50. A supplied object accepts only `limit` and `cursor`; `limit` is an
integer from 1 through 100. The result is:

```json
{"result":{"items":[{"public_id":"<22-character Base64url>",
                      "username":"administrator",
                      "display_name":"First Administrator",
                      "active":true,
                      "mfa_required":false}],
           "next_cursor":null},
 "correlation_id":"<opaque-server-value>"}
```

`display_name` is a string or `null`. Each item contains exactly these five
account fields. It contains no password verifier, credential state, temporary
credential value or metadata, MFA factor, session value, internal state
identifier, or Audit Reference Identifier.

The cursor is canonical unpadded URL-safe Base64 and opaque to the client. It
is scoped to this exact route and API version and carries the last returned
immutable unique username as its keyset position. A cursor from another route
or version, a padded or otherwise noncanonical encoding, an over-bound value,
or a position that is not present is `bad_request`. A page sets
`next_cursor` to a string only when another item remains; otherwise it is
`null`. Accounts are not deleted and usernames are immutable, so a previously
issued valid position remains stable under the supported account semantics.

The view body is exactly:

```json
{"public_id":"<22-character Base64url>"}
```

The Account Public Identifier is the account-administration target. Its public
representation is exactly 22 unpadded Base64url characters encoding the
account's nonzero independent 128-bit identifier. It is not an account state
identifier, Audit Reference Identifier, session value, or credential. The
lowercase hexadecimal `account_id` remains only in the additive authentication
session result for compatibility and is not accepted here. A successful view
returns the same five-field projection directly as `result`; an unknown valid
identifier is `404 not_found`.

Both bodies use strict JSON. Unknown or duplicate members, missing required
members, wrong types, trailing content, malformed identifiers or cursors, and
oversized input are `bad_request`. The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, body, schema, identifier, or cursor | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any live authorization denial | `403 Forbidden`, `authorization_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Unknown valid view identifier | `404 Not Found`, `not_found` |
| Persistence, integrity, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

Every code is carried by the typed error envelope with a Server-generated
correlation identifier. No rejection carries a message, field path, lookup
detail, dependency name, or supplied value. Neither route is mounted on a
Pre-Operational Surface.

### Client Module Identity

Because Client Modules share routes, a request does not name its Client Module
in its path. The Server determines the Client Module from the authenticated
session, which records the Client Module the session was established for. A
request to create a session declares its Client Module explicitly through the
login route's `client_module` field. In the current build, the Web UI Client
Module is the only registered value the login route accepts; a login request
naming any other value is denied.

This preserves per-Client-Module Group grants. Grants apply only to
authenticated requests, and pre-operational contracts run before any Human User
exists, so no grant applies to them.

### Lifecycle Reconciliation

`PUT /api/v1/lifecycle/reconciliation` lets a browser that held a completed
**[Init](../../glossary.md#states-and-requests)** or
**[Restore](../../glossary.md#states-and-requests)** submission confirm,
after the fact, whether that exact submission is the one this deployment
completed. It is mounted once normal operation begins and is neither a
pre-operational contract nor part of `/api/v1/auth/`: it requires no
authenticated session of its own, and it neither reads nor changes the session
`/api/v1/auth/session` validates. A caller may reconcile before ever signing
in, and calling it changes no session state.

The request body must be exactly one JSON object:

```json
{"reconciliation_capability":"<opaque token>"}
```

`reconciliation_capability` is the high-entropy opaque value the completing
Init recovery-key response or Restore ticket response delivered; it carries no
other meaning and is never derived from a session, a correlation identifier,
or any other value. Validation is strict: an unknown field, a duplicate key, a
missing member, a malformed token, trailing content, and an oversized body are
all request errors, using the same bounded opaque-token shape as every other
issued token in this contract. The request enforces the same same-origin,
`Host`, and `X-Weavelit-CSRF` precondition every other browser-reachable route
in this contract enforces, evaluated before body parsing.

A submitted capability's domain-separated digest is compared, in constant
time, only against the single digest the completing Init or Restore committed.
Every response is fixed and reflects none of the submitted capability, a
deployment identifier, or any other value that could distinguish the cause of
a non-match:

| Condition | Response |
| --- | --- |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `{"error":"method_not_allowed"}` |
| Failed `Origin`, `Host`, or `X-Weavelit-CSRF` precondition | `403 Forbidden`, `{"error":"request_origin_denied"}` |
| Malformed, oversized, or schema-invalid body; wrong, missing, or duplicate `Content-Type`; unsupported or duplicate `Accept` | `400 Bad Request`, `{"error":"bad_request"}` |
| Submitted capability does not match the one live reconciliation digest | `404 Not Found`, `{"error":"not_found"}` |
| Reconciliation store unavailable | `503 Service Unavailable`, `{"error":"service_unavailable"}` |

A `404` is not evidence of failure: a wrong capability, a capability for a
deployment that never completed, and a capability superseded by a later
Restore's own digest all answer identically, so a caller cannot distinguish
the cause from this response alone. The
[Server Init Design](../lifecycle/init/init-design.md#recovery-key-delivery-and-finalization)
and the
[Server Restore Design](../lifecycle/restore/restore-design.md#two-request-submission-protocol)
own where each capability originates and how its digest is persisted and, for
Restore, atomically replaced.

## Future Audit Terminal Recovery Administration

The future **[Administration Plane](../../glossary.md#applications-and-interfaces)**
configuration contract owns Log Module binding generations, ordinary change
sequencing, retained-binding status, degraded Audit-completeness presentation,
and the terminal recovery user interface. This version 1 contract does not yet
define a route, method, request body, response schema, or client implementation
for those functions.

An ordinary configuration change must retain every binding version referenced
by a pending terminal obligation. A future supersession request may address
only the exact oldest valid active obligation after binding repair proves its
destination permanently unavailable. The handler must consume an
action-scoped authorization proof created by fresh password reauthentication
for the exact current session and fresh TOTP verification when enrolled,
separate explicit confirmation bound to the displayed original and replacement,
and a successful replacement Audit preflight proof. It must not accept boolean
fields as substitutes for any proof.

The future status surface must distinguish active obligations from retained
late-delivery obligations and present degraded completeness as an integrity
exception rather than success. Client-visible output and the new Audit action
must omit destination errors, settings, credentials, authentication evidence,
and confirmation content. It must not offer Restore, System Log creation,
replacement delivery, Correction, or manual acknowledgement as a way to clear
the original.

## Result And Error Representation

A successful response carries a structured result and a Server-generated
correlation identifier:

```json
{"result":{},"correlation_id":"<opaque-server-value>"}
```

A failed response carries a stable code and the same correlation identifier:

```json
{"error":"<stable_code>","correlation_id":"<opaque-server-value>"}
```

An error carries no message, field path, stack trace, dependency name, or other
detail. Codes are stable, redacted, and dependency-neutral, so a caller cannot
distinguish causes the Server deliberately treats as equivalent. The correlation
identifier is the only supported way to relate a client-visible failure to
Server-side records.

### Authorization Denial

Every request to `/api/v1/user/` or `/api/v1/administration/` that a
[Server Authorization Design](../authorization/authorization-design.md)
decision denies receives HTTP `403` with exactly:

```json
{"error":"authorization_denied","correlation_id":"<opaque-server-value>"}
```

The body is byte-identical whatever the denial cause: an inactive
**[Human User](../../glossary.md#identities-and-access)**, a disabled or
uncatalogued **[Client Module](../../glossary.md#applications-and-interfaces)**,
**[Service Module](../../glossary.md#applications-and-interfaces)**, or
**[Operation](../../glossary.md#applications-and-interfaces)**, and every
missing grant are indistinguishable in the response. Only the correlation
identifier varies, and it is the same value the corresponding System Log
denial record carries.

This is a distinct contract from the `401` session-validation failure a route
returns when it cannot tell who is asking, documented in the
[Server Authentication Design](../authentication/authentication-design.md).
Authorization never runs unless session validation has already succeeded, so a
given request is never denied by both contracts; the two are never
alternatives for one request.

### Response Profiles

The Server uses two response profiles.

The fixed profile serves the frozen pre-operational lifecycle routes. It emits
only compile-time response bodies drawn from an allowlist, sets no cookies, and
bounds bodies far below any dynamic payload. It remains unchanged.

The typed profile serves every other route. It serializes structured results,
carries correlation identifiers, and is the only profile permitted to emit the
approved session and cross-site request forgery cookies. Both profiles enforce
their own bounds; neither can emit a body the other is responsible for.

The typed profile's bound is derived from the envelope's own maxima rather than
inherited from the fixed profile: a stable code, a correlation identifier at its
canonical bound, and the result fields a route may return. The Server re-checks
each serialized envelope against that bound and redacts rather than truncating,
so a route cannot emit a partial envelope that still parses as valid JSON. The
login route emits the approved session-issuing cookie effect and the logout
route emits the approved session-clearing cookie effect; every other route,
including session validation, emits no cookie. Each cookie effect is itself
bounded and fails closed rather than emitting a partial `Set-Cookie` line; see
the [Server Authentication Design](../authentication/authentication-design.md#cookie-emission)
for the exact bound.

#### Response Buffer Clearing

The typed profile serializes each envelope into a controlled, fixed-capacity
application buffer. The listener transfers that allocation to the response
body owner and clears it when the final body clone is released, including after
a successful write, write or shutdown failure, and response-timeout
cancellation.

When a typed response carries the approved cookie effect, the cookie values,
rendered `Set-Cookie` lines, and complete listener-composed response head are
also controlled application-owned buffers. The listener composes that head from
borrowed framing and rendered-line parts before it writes, holds the resulting
byte owner through the head write, body write, and connection shutdown, then
clears it on the same normal, error, or timeout release paths. This is a
guarantee only for those controlled application buffers. Copies held by TLS,
the kernel or network transport, and the allocator are outside application
control.

#### Request-Head Buffer Clearing

The **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
listener owns each raw request head in a controlled allocation, including the
single overflow-sentinel byte that detects an aggregate-limit rejection. On a
completed parse, each HeaderValue keeps a validated shared range of that owner,
so the allocation remains until the final non-empty HeaderValue, HeaderMap, or
Request clone releases it. Malformed, oversized, incomplete, framing, parser,
timeout, and cancellation paths release and clear the same owner without
changing request classification, limits, or rejection ordering.

The guarantee excludes copies held by TLS, the kernel or network transport,
the allocator, URI parsing, or a consumer after it copies a value. It also does
not extend to bodies and leaves the encrypted Restore-artifact exclusion in
[Secret Request-Body Handling](#secret-request-body-handling) unchanged.

#### Future Credential-Issuance Response

Future account-create and password-reset contracts may return a typed
credential-issuance response containing a bounded temporary-password field of
24 unpadded Base64url characters and the existing bounded `correlation_id`.
This is a future wire-boundary contract only; it does not create a route or
imply that either contract is implemented. The response is constructed and
accepted into its bounded, zeroizing response buffer before the account
mutation commits, and plaintext is returned only after that pre-mutation
construction succeeds and the mutation succeeds. It carries
`Cache-Control: no-store` and must not create a redirect, URL, cookie, or
browser-storage entry. The response has no later retrieval or view operation.

Credential issuance is deliberately non-retryable under the generic
[Idempotency](#idempotency) policy. A lost or indeterminate response receives
no automatic retry or repeated plaintext; it requires a new reset. Stable
payload-free errors remain dependency-neutral.

#### Producer Obligations

Failing closed is the envelope's last defence, not a route's error-handling
strategy. A route that fails closed after taking an effect the caller cannot
retry converts a formatting problem into a permanent one, so a producer of a
bounded result field carries two obligations.

First, a producer fits its value to the bound the envelope enforces instead of
handing over a value the envelope will reject. Where a field's own input bound
and the envelope's bound are set independently, the producer is responsible for
reconciling them; it may not assume an accepted input can always be rendered.

Second, a producer builds and accepts every bounded field into its
response-bearing type before it takes any single-use or otherwise irreversible
effect. Nothing between that effect and the composed response may refuse. A
caller then either receives the whole result or receives a refusal that
consumed nothing and can be retried.

The one-time MFA provisioning result is the worked example of both: its
`provisioning_uri` is fitted to the typed profile's provisioning bound and
accepted into its bounded type before the confirming enrollment continuation is
issued. The bound itself is unchanged, and no account name can make an
enrollment unopenable. The
[TOTP Module Design](../../mfa-modules/totp-module-design.md#provisioning-uri-construction)
owns how the URI is fitted.

## Secret Request-Body Handling

Some request bodies carry plaintext secret material: the login route's
`password`, the second-factor routes' one-time `code` and `continuation`
values, the session-bearing enrollment route's re-verified `password`, both
Init requests' initial credential and recovery-key proof material, and the
Restore recovery key. For every one of them the shared
`weavelit-module-client` crate takes sole ownership of the collected request
buffer before deserializing it and clears that buffer when its ownership ends.

The clear runs on release rather than at one exit point, so the accepted path,
every rejection path, and any path added later clear the same buffer. Parsed
secret values are then held only in clearing wrappers for the rest of the
request. This changes no bound, no status, no error code, and no rejection
ordering: an oversized, malformed, or unparseable body is refused exactly as it
was before.

If sole ownership of the collected buffer cannot be taken, the surface clears
the copy it does own and leaves the shared original to its owner; that fallback
is never a rejection. This handling is defense in depth, not a whole-process
guarantee: the transport layer's own read buffers are outside this Module, so
what is guaranteed is that this Module retains no uncleared copy, not that no
copy exists anywhere in the process.

Bodies that carry no plaintext secret are deliberately excluded, including the
encrypted Restore artifact, which discloses nothing on its own.

## Pagination

A collection response is cursor-paginated. A request MAY supply `limit` and
`cursor`. `limit` defaults to 50 and MUST NOT exceed 100. A cursor is opaque and
carries no client-interpretable meaning. A collection result contains `items`
and a nullable `next_cursor`.

## Idempotency

Version 1 defines no global idempotency-key store. Existing contracts do not
require one: Application Database selection is naturally idempotent, Init and
Restore are each deliberately non-retryable once their checkpoint exists, and
each authentication attempt is a distinct event.

A future unsafe write Operation MUST declare its own duplicate protection before
its route is added. Introducing keyed protection for a specific Operation is an
additive change to that Operation's contract.

## Compatibility

Version 1 is additive-only. The Server MAY add result fields, add routes, and
add stable error codes. A client MUST ignore result fields it does not
recognize.

Removing a field, changing the meaning of a field, changing route or status
semantics, or changing a stable error code requires `/api/v2/`.

The Server and the separately packaged
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)** are compatible
by API major version rather than by release version. Any CLI interoperates with
any Server that serves the same API major version. A CLI that requires a route
the Server does not expose reports a stable `unsupported_server_api` result. The
Server exposes no version-negotiation endpoint and discloses no release version
for this purpose.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server Authentication Design](../authentication/authentication-design.md)
- [Server Authorization Design](../authorization/authorization-design.md)
- [Audit Terminal Binding Retention And Supersession Decision](../../log-modules/audit-terminal-binding-retention-decision.md)
- [Server Lifecycle Design](../lifecycle/lifecycle-design.md)
- [Server Init Design](../lifecycle/init/init-design.md)
- [Server Restore Design](../lifecycle/restore/restore-design.md)
- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Init Surface](../../client-modules/web-ui/pre-operational-init-design.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Glossary](../../glossary.md)
- [Temporary Password Disclosure Decision](../authentication/temporary-password-disclosure-decision.md)
