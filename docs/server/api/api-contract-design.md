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
    values differ. This continuation-bearing response emits
    `Cache-Control: no-store`.
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
    that confirms this exact enrollment. This provisioning response emits
    `Cache-Control: no-store`.
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

### Group Administration

The Group administration surface contains exactly ten strict `PUT` routes:

| Route | Body | Result |
| --- | --- | --- |
| `/api/v1/administration/groups/list` | optional `limit` and `cursor` | `items` and nullable `next_cursor` |
| `/api/v1/administration/groups/view` | `public_id` | one Group projection |
| `/api/v1/administration/groups/create` | `name` and nullable or omitted `description` | the created Group projection |
| `/api/v1/administration/groups/update` | `public_id`, `name`, and nullable or omitted `description` | the resulting Group projection |
| `/api/v1/administration/groups/delete` | `public_id` and `grant_mutation_step_up_ticket` | the deleted `public_id` |
| `/api/v1/administration/groups/members/list` | `group_public_id`, optional `limit`, and optional `cursor` | safe Account projection `items` and nullable `next_cursor` |
| `/api/v1/administration/groups/members/change` | `group_public_id`, `account_public_id`, `present`, and `grant_mutation_step_up_ticket` | `account` safe projection and resulting `present` state |
| `/api/v1/administration/groups/grants/list` | `group_public_id`, optional `limit`, and optional `cursor` | canonical grant `items` and nullable `next_cursor` |
| `/api/v1/administration/groups/grants/change` | `group_public_id`, structured `grant`, `present`, and `grant_mutation_step_up_ticket` | canonical `grant` and resulting `present` state |
| `/api/v1/administration/catalog` | empty or absent object | compiled-in `client_modules`, `service_modules`, and `operations` |

Every request requires the ordinary session, exact same-origin `Origin` and
`Host`, session `X-Weavelit-CSRF`, JSON media types, live Web UI Client Module
access, and effective Server Administration Permission used by account
administration. Bodies reject unknown or duplicate members, trailing content,
wrong types, control characters, over-bound values, malformed identifiers,
cursors, or tickets as `bad_request`.

The Group Public Identifier is an independent nonzero random 128-bit value
encoded as exactly 22 canonical unpadded Base64url characters. It is the only
Group target accepted or returned. A projection contains exactly `public_id`,
`name`, and nullable `description`; it contains no state identifier, Audit
Reference Identifier, membership, grant, or count.

Member projections use the Account Public Identifier and the same exact five
safe fields as Account administration: `public_id`, `username`, nullable
`display_name`, `active`, and `mfa_required`. A direct grant is exactly one of
`{"type":"client_module","value":"<catalog name>"}`,
`{"type":"service_module","value":"<catalog name>"}`,
`{"type":"operation","value":"<catalog name>"}`, or
`{"type":"server_administration"}`. The server-administration variant accepts
no `value`; the other variants require one bounded catalog name. No route
accepts a state identifier, Audit Reference Identifier, free-form grant kind,
component enablement state, or confirmation member.

List uses limits 1 through 100 and default 50. Its opaque cursor is scoped to
this route and API version and carries the last returned unique Group name. A
malformed, cross-route, noncanonical, over-bound, or currently absent position
is `bad_request`. Reads produce no Audit record.

Member and grant lists use the same limit bounds and distinct route-scoped
opaque cursors. Member order is deterministic by username and Account Public
Identifier; grant order is the canonical structured-grant order. The catalog
contains at most 256 strictly ordered values per component kind, comes only
from the Server's compiled-in component inventory, and contains no enablement,
configuration, credential, provider, state, or Audit data. An unknown valid
Group target is `404 not_found`; an existing Group with no associations returns
an empty page.

Create produces an empty Group. Update replaces name and nullable description;
an exact no-op returns the current projection and produces no Audit record. A
duplicate name is `409 conflict`. An unknown valid target is `404 not_found`.

Delete requires the five-minute TOTP step-up family `grant_mutation`. The
shared step-up route accepts `{"family":"grant_mutation","code":"123456"}`.
Its ticket is bound to that family and cannot substitute for `mfa_policy`.
Delete succeeds only when the Group has no memberships and no direct grants. A
nonempty Group is `409 conflict` without a count or association kind. Invalid
proof is `403 grant_mutation_denied`. Clients MUST NOT automatically retry
step-up or deletion after an unreadable or unreported outcome.

Membership and direct-grant changes require the same family-bound five-minute
proof. The Account target is always an Account Public Identifier. A Client
Module, Service Module, or Operation grant must name a value in the compiled-in
catalog; an unknown valid value is `404 not_found` before Audit or mutation.
An already-present or already-absent association returns the requested safe
result and produces no Audit record. A changed association uses the existing
atomic Group mutation and Audit sequence. A removal that would eliminate the
last active effective Administrator is `409 conflict` with no account, Group,
grant, count, or policy detail. Missing, malformed, expired, cross-session, or
cross-family proof is the same `403 grant_mutation_denied`. Clients MUST NOT
automatically retry step-up or a member or grant change after an unreadable or
unreported outcome.

### Account Status Administration

The account-status surface contains exactly one route:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/accounts/status` | `PUT` | The target account's safe projection after the requested active state is confirmed. |

The route requires the same ordinary validated session, exact same-origin
`Origin` and `Host`, session `X-Weavelit-CSRF` value, JSON media types, live Web
UI Client Module access, and effective Server Administration Permission as the
account-read routes. It requires no credential-issuance ticket, password
reauthentication, TOTP code, or current-session MFA step-up. Its body is exactly:

```json
{"public_id":"<22-character Base64url>","active":false}
```

`public_id` is the Account Public Identifier defined by the read contract, and
`active` is the desired state. The route accepts no State Identifier, Audit
Reference Identifier, caller identity, session value, confirmation field,
password, TOTP code, or credential-issuance ticket. Unknown or duplicate
members, missing members, wrong types, trailing content, malformed identifiers,
and oversized input are `bad_request`.

Success returns the same exact five-field safe account projection as view. The
returned `active` value equals the requested state. A request for the target's
current state succeeds with that projection and produces no Audit record. A
committed disable revokes every target session, including the request's own
session when an Administrator disables their own account. The successful
self-disable response is still returned; the revoked cookies are
`session_invalid` on the next request. Re-enabling an account creates no session
and restores none of its revoked sessions.

A changed status follows the Server's consequential-operation Audit sequence.
The response does not expose whether terminal delivery was immediate or remains
pending for bounded recovery. A client MUST NOT retry automatically after an
unreported or malformed outcome because the mutation may already have
committed.

The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, body, schema, or identifier | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any initial or final live authorization denial | `403 Forbidden`, `authorization_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Unknown valid target identifier | `404 Not Found`, `not_found` |
| Persistence, integrity, Audit readiness, target staleness, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

Every rejection uses the typed error envelope and reveals no target detail,
mutation phase, session count, Audit state, or supplied value. The route is not
mounted on a Pre-Operational Surface.

### Account MFA Policy Administration

The account MFA-policy surface contains exactly three routes:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/step-up/totp` | `PUT` | One reusable short-lived ticket for the requested public step-up family. |
| `/api/v1/administration/accounts/mfa-requirement` | `PUT` | The target account's safe projection after its MFA requirement is confirmed. |
| `/api/v1/administration/accounts/mfa-reset` | `PUT` | The target account's safe projection after its TOTP enrollment is absent. |

All three routes require an ordinary validated session, exact same-origin
`Origin` and `Host`, the session's `X-Weavelit-CSRF` value, JSON media types,
live Web UI Client Module access, and the effective Server Administration
Permission. Each body uses strict JSON; an unknown or duplicate member,
missing member, wrong type, trailing content, malformed identifier, ticket, or
code, or oversized input is `bad_request`.

The TOTP step-up body is exactly one of:

```json
{"family":"mfa_policy","code":"123456"}
```

```json
{"family":"grant_mutation","code":"123456"}
```

`code` is exactly six decimal digits. `mfa_policy` and `grant_mutation` are the
two public families in this contract. `mfa_policy` is consumed only by account
MFA-requirement and enrollment-reset actions. `grant_mutation` is consumed only
by existing-Group membership and direct-grant changes and empty Group deletion.
Any other family is `bad_request` and exposes no family-specific route or
denial detail.
The Server verifies the code only for the exact authenticated session and its
current enrolled TOTP factor. Verification atomically rechecks session
liveness, actor activity, factor ownership, TOTP Module enablement, and replay
state before advancing the watermark. It issues no cookie and creates or
rotates no session. Success returns:

```json
{"result":{"totp_step_up_ticket":"<43-character canonical Base64url>"},
 "correlation_id":"<opaque-server-value>"}
```

This ticket-bearing response emits `Cache-Control: no-store`.

The ticket contains 256 bits of operating-system randomness. The process
retains only its domain-separated digest and the private `MfaStepUpProof` in a
bounded 64-entry memory store. The ticket is reusable for matching actions in
its selected family until the proof's exact five-minute monotonic expiry; the
exact expiry instant is invalid. It is bound to the issuing actor, session,
Client Module, factor, and selected public family. A restart invalidates it. A
ticket for one public family cannot authorize the other family, and neither can
substitute for the single-use credential-issuance ticket.

The requirement body is exactly:

```json
{"public_id":"<22-character Account Public Identifier>",
 "required":true,
 "totp_step_up_ticket":"<ticket>"}
```

The reset body is exactly:

```json
{"public_id":"<22-character Account Public Identifier>",
 "totp_step_up_ticket":"<ticket>"}
```

Neither action accepts a confirmation boolean, password, TOTP code, actor,
session, factor, requirement snapshot, or internal identifier. Each request
performs new session validation and live Administration authorization, then
checks the ticket through the private action gate. The final writer again
checks the exact issuer session, actor activity, Client Module, TOTP factor,
Module enablement, target public identity, target requirement, and target
factor snapshot.

A requirement no-op returns the safe projection and creates no Audit record.
Changing to required revokes every target session so no password-only session
survives the new requirement. Changing to optional preserves current target
sessions. Enrollment reset with no current TOTP factor is likewise a no-op.
A committed reset deletes the factor and its replay watermark, preserves the
password and MFA requirement, and revokes every target session. A target that
remains required must enroll before receiving another usable session.
Requirement changes and resets use distinct Audit events. Business state and
the selected terminal obligation commit atomically.

Both actions return the same exact five-field safe account projection as the
account-view route. A successful self-require or self-reset response is returned
after commit even when the transaction revoked the requesting session; the next
request is `session_invalid`. No response exposes a factor, watermark, affected
session count, ticket, code, proof, or terminal-delivery state.

The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, body, schema, family, identifier, ticket, or code shape | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any live Administration Plane authorization denial | `403 Forbidden`, `authorization_denied` |
| TOTP, replay, factor, Module, ticket, proof binding, expiry, target staleness, or final policy recheck denied | `403 Forbidden`, `mfa_policy_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Unknown valid target identifier | `404 Not Found`, `not_found` |
| Persistence, integrity, time, randomness, Audit readiness, capacity, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

Every rejection is payload-free beyond its stable code and correlation
identifier. A client MUST NOT automatically repeat step-up, requirement, or
reset after an unreported or malformed outcome. It must discard the code and
ticket, report the outcome as unknown, and require a manual refresh before
another MFA policy action.

### TOTP Module Enablement Administration

TOTP Module enablement uses two specialized routes:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/mfa-modules/totp/enablement/preview` | `PUT` | Current and desired enablement, affected enrolled users, and one single-claim preview credential. |
| `/api/v1/administration/mfa-modules/totp/enablement/apply` | `PUT` | The committed enablement and affected-user count. |

Both routes require an ordinary validated session, exact same-origin `Origin`
and `Host`, the session's `X-Weavelit-CSRF` value, JSON media types, live Web UI
Client Module access, and the effective Server Administration Permission.
Enablement is an ordinary `ComponentEnablementChange(MfaModule, "totp",
desired_state)` Administration action. It is deliberately not the `MfaPolicy`
or `GrantMutation` step-up family: requiring proof from the disabled TOTP
Module would make re-enablement impossible. Neither MFA-policy nor
credential-issuance tickets can substitute for the specialized preview
credential.

The preview body is exactly:

```json
{"enabled":false}
```

Success returns:

```json
{"result":{"module":"totp","current_enabled":true,
 "desired_enabled":false,"affected_users":2,
 "totp_enablement_preview":"<43-character canonical Base64url>"},
 "correlation_id":"<opaque-server-value>"}
```

This preview-credential response emits `Cache-Control: no-store`; the applied
result and every rejection do not.

The credential contains 256 bits of operating-system randomness. The process
retains only its domain-separated SHA-256 digest in a bounded 64-entry memory
store for five minutes measured by a monotonic clock; the exact expiry instant
is invalid and restart invalidates every entry. It binds the exact actor,
session digest, Client Module, desired state, and retained preview. Apply claims
it once before consequential work, so replay, concurrent duplicate use, an
actor, session, Client Module, or desired-state mismatch, and expiry are all
the same conflict. The plaintext credential MUST NOT enter a rendered page,
URL, cookie, browser storage, Log record, or Audit record.

The apply body is exactly:

```json
{"enabled":false,"totp_enablement_preview":"<preview>"}
```

A missing preview member reaches the claim decision and is a conflict; a
present noncanonical value is malformed input. The transaction recounts
enrolled Human Users against the retained preview. A changed count commits only
the denied Audit terminal obligation and changes no enablement or session
state. A matching disable preserves every factor and MFA requirement, writes
the disabled state, and revokes every session belonging to an enrolled account
atomically. Enrolled optional accounts can later authenticate by password while
disabled; required accounts are denied until the Module is enabled and a
verifiable factor is available. Enablement revokes no session.

Every applied business result is rendered as success after the enablement and
its terminal obligation commit atomically:

```json
{"result":{"module":"totp","current_enabled":false,"affected_users":2},
 "correlation_id":"<opaque-server-value>"}
```

The response is identical whether exact terminal Audit delivery was immediately
acknowledged or remains pending for internal recovery. It exposes no Audit
delivery state and does not invite a retry. A client treats the safe `200`
result as committed success and MUST NOT retry apply automatically after an
unreported or malformed outcome because the mutation may already have
committed. It probes the ordinary session route after every valid success; a
successful self-disable that revoked the caller returns the application to
sign-in without another mutation.

The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, media, schema, or present preview shape | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any live Administration Plane authorization denial | `403 Forbidden`, `authorization_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Missing, expired, replayed, or mismatched preview, or stale affected-user count | `409 Conflict`, `conflict` |
| Randomness, capacity, persistence, integrity, time, pre-commit Audit readiness or delivery, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

### Log Configuration Administration

Existing Log Module configurations use three specialized routes:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/log-configurations/list` | `PUT` | One cursor page of safe current configuration projections. |
| `/api/v1/administration/log-configurations/view` | `PUT` | One safe current projection selected by unique configuration name. |
| `/api/v1/administration/log-configurations/change` | `PUT` | The primary configuration's safe projection after a committed change or exact no-op. |

All routes use the same ordinary-session, same-origin, CSRF, JSON response,
live Client Module, and Server Administration Permission requirements as TOTP
enablement. They accept no generic component or Operation target, internal
identifier, generation, destination path, credential, protected setting,
terminal obligation, or record identifier.

List uses the existing cursor pattern: an absent body or an exact object with
optional `limit` and `cursor`, a default limit of `50`, and an inclusive maximum
of `100`. The cursor is opaque, route-scoped, and bound to the exact prior
configuration name. Results are strictly ordered by unique configuration name.
View accepts exactly:

```json
{"configuration_name":"primary"}
```

Each safe projection is exactly:

```json
{"configuration_name":"primary","module":"sqlite","enabled":true,
 "settings":[],"assigned_log_types":["system","audit"]}
```

`settings` is the complete canonically key-ordered set of non-secret,
non-path settings declared by that compiled-in Log Module.
`assigned_log_types` is ordered as System then Audit. The projection omits
application state and Audit identifiers, generations, local or remote paths,
credentials and protected settings, Audit terminal state, retention and purge
state, and Log records.

Change names one existing primary configuration and supplies at least one of
`enabled`, complete `settings`, or complete `assignments`. When present,
`assignments` contains exactly one System and one Audit entry in canonical
order, each shaped as `{"log_type":"audit","configuration_name":"primary"}`.
Unknown or duplicate members, duplicate setting keys or Log Types, partial
settings or assignments, unsupported settings, and noncanonical ordering are
rejected. The Server resolves every public name to the current internal
configuration identity only after session validation and authorization.

An exact no-op returns the current safe projection and creates no Audit record.
Otherwise the existing immutable-generation workflow validates the complete
result against the module catalog, preflights every assigned destination,
delivers the Attempt, and atomically commits the selected generations,
assignments, pointers, and terminal obligation. A stale generation or topology
commits only the denied terminal and is a conflict. An applied change returns
the ordinary safe `200` projection whether terminal Audit delivery was
immediately acknowledged or remains pending for internal recovery. The response
exposes no Audit delivery state. A client MUST NOT retry after an unreported or
malformed outcome because the change may already have committed.

The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, media, body, schema, cursor, name, unsupported or incomplete setting input | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any live Administration Plane authorization denial | `403 Forbidden`, `authorization_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Unknown existing primary configuration name | `404 Not Found`, `not_found` |
| Unknown assignment name, invalid or stale topology, generation exhaustion, or stale preparation | `409 Conflict`, `conflict` |
| Catalog, preflight, pre-commit Audit delivery, persistence, integrity, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

### Application Database Backup Download

The normal operational **[Administration Plane](../../glossary.md#applications-and-interfaces)**
contains exactly one encrypted **[Application Database](../../glossary.md#applications-and-interfaces)**
backup route:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/backups/create` | `PUT` | One downloaded encrypted backup artifact. |

The route requires an ordinary validated session, exact same-origin `Origin`
and `Host`, the session's `X-Weavelit-CSRF` value, a live
**[Client Module](../../glossary.md#applications-and-interfaces)** that
declares the Administration Plane, a grant to that Client Module, and the
effective **[Server Administration Permission](../../glossary.md#identities-and-access)**.
The Server evaluates the Client Module grant and authorization from the
authenticated session and live Server state; the request supplies no identity,
Client Module, grant, or permission claim. The route is not mounted on a
Pre-Operational Surface.

The request has no body and MUST NOT carry `Content-Type`. `Accept` MAY be
absent or MUST be exactly `application/octet-stream`. It accepts no
`Idempotency-Key` or other request field.

On success, the Server returns `200 OK` with raw encrypted backup bytes, not a
JSON envelope. It sends exactly these response headers in addition to normal
HTTP framing:

| Header | Value |
| --- | --- |
| `Content-Type` | `application/octet-stream` |
| `Content-Disposition` | `attachment; filename="weavelit-backup.wlitbackup"` |
| `Cache-Control` | `no-store` |
| `X-Content-Type-Options` | `nosniff` |
| `X-Weavelit-Correlation-Id` | Server-generated correlation identifier |

The success response sets no cookie, sends no CORS header, supplies no
artifact URL or resource identifier, and makes no `Content-Length` promise.
It exposes no backup identifier or artifact detail. A successful creation
records `lifecycle.backup.created` through the existing accountability
semantics using the same correlation identifier; this route does not define
the backup's creation, snapshot, encryption, storage, or cleanup behavior.

The binary success exception does not change the typed error profile. Every
failure carries the existing typed JSON error envelope and a Server-generated
correlation identifier, with only these route-specific outcomes:

| Condition | Response |
| --- | --- |
| Malformed headers, a body, `Content-Type`, or unacceptable `Accept` | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check, or missing or mismatched `X-Weavelit-CSRF` | `403 Forbidden`, `request_origin_denied` |
| Any live Administration Plane authorization denial | `403 Forbidden`, `authorization_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Required service unavailable | `503 Service Unavailable`, `service_unavailable` |

Listener loss, timeout, or an unreadable response is indeterminate. Only a
fully received `200 OK` artifact proves success. A client MUST NOT
automatically retry, resume, reconcile, or retrieve a prior artifact after an
indeterminate outcome. It may make a fresh explicit request, which is a
distinct backup creation and may create another backup. Version 1 provides no
later backup-retrieval route.

This is an additive `/api/v1/` route. Its route, request, response, status,
header, retry, and error semantics are subject to the version-1 compatibility
rule; an incompatible change requires `/api/v2/`.

### Account Credential Issuance

The account credential-issuance surface contains exactly three routes:

| Route | Method | Result |
| --- | --- | --- |
| `/api/v1/administration/step-up/credential-issuance` | `PUT` | One short-lived credential-issuance ticket after fresh assurance. |
| `/api/v1/administration/accounts/create` | `PUT` | One committed local account and its temporary password. |
| `/api/v1/administration/accounts/reset-password` | `PUT` | One committed password reset and its temporary password. |

All three routes require an ordinary validated session, exact same-origin
`Origin` and `Host`, the session's `X-Weavelit-CSRF` value, an unparameterized
`Content-Type: application/json`, an acceptable JSON response media type, live
Web UI Client Module access, and the effective Server Administration
Permission. Each request uses strict JSON: unknown or duplicate members,
missing required members, wrong types, trailing content, malformed bounded
values, and oversized input are `bad_request`.

The credential-assurance body is exactly:

```json
{"password":"current password","totp_code":"123456"}
```

`password` is required, nonempty, and at most 1,024 bytes. `totp_code` is
optional in the wire schema and, when present, is exactly six decimal digits.
The Server accepts it only when it matches the authenticated account's current
enrollment requirements. Success returns:

```json
{"result":{"credential_issuance_ticket":"<43-character canonical Base64url>"},
 "correlation_id":"<opaque-server-value>"}
```

The ticket represents 256 bits of Server-generated entropy. Its authentication
binding, lifetime, single-claim behavior, and distinction from an MFA policy
step-up are owned by the
[Server Authentication Design](../authentication/authentication-design.md#credential-issuance-assurance).

The account-create body is exactly:

```json
{"username":"new-user",
 "display_name":"New User",
 "credential_issuance_ticket":"<ticket>"}
```

`username` is required. `display_name` is optional and MUST be omitted rather
than sent as `null` when absent. Each supplied name is nonempty, contains no
control character, and is at most 256 bytes. The password-reset body is
exactly:

```json
{"public_id":"<22-character Account Public Identifier>",
 "credential_issuance_ticket":"<ticket>"}
```

The create and reset routes each claim the submitted ticket for exactly one
attempt and bind it to the separately authorized action. A successful create or
reset returns only:

```json
{"result":{"public_id":"<22-character Account Public Identifier>",
           "temporary_password":"<24-character canonical Base64url>"},
 "correlation_id":"<opaque-server-value>"}
```

The temporary password is disclosed only in the originating successful
response. No route retrieves, reconstructs, or redisplays it. The response sets
no cookie and creates no redirect or URL. The Server prepares the bounded
secret-bearing typed result before mutation and transfers the plaintext into
the response only after the final writer commits.

The credential-issuance ticket response and the originating create or reset
response each emit `Cache-Control: no-store`. The typed response profile still
has no arbitrary response-header channel: a closed internal secret-disclosure
effect selects only that fixed directive, and no route supplies a header name
or value. The full approved effect inventory and its exclusions are owned by
the [Security Model](../../security-model.md#secret-disclosure-cache-control).
The Server persists neither ticket plaintext nor temporary-password plaintext,
and clients MUST treat both responses as sensitive, use no-store request
behavior, and keep their values out of URLs, cookies, browser storage, logs,
and later view state.

The stable rejection contract is:

| Condition | Response |
| --- | --- |
| Malformed headers, body, schema, name, identifier, ticket, or code shape | `400 Bad Request`, `bad_request` |
| Missing, malformed, unknown, expired, mismatched, or restricted session | `401 Unauthorized`, `session_invalid` |
| Failed exact origin or host check | `403 Forbidden`, `request_origin_denied` |
| Any live Administration Plane authorization denial | `403 Forbidden`, `authorization_denied` |
| Password, enrollment evidence, factor state, ticket claim, binding, or final assurance recheck denied | `403 Forbidden`, `credential_issuance_denied` |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `method_not_allowed` |
| Existing create username | `409 Conflict`, `conflict` |
| Unknown valid reset target | `404 Not Found`, `not_found` |
| Persistence, integrity, time, randomness, Audit readiness, or trusted composition unavailable | `503 Service Unavailable`, `service_unavailable` |

Every rejection uses the typed error envelope with only its stable code and a
Server-generated correlation identifier. A client may distinguish a reported
typed refusal from an indeterminate outcome, but MUST NOT render or infer a
reason within either category. Transport loss, the listener's
`504 gateway_timeout`, an unreadable response, or an invalid success envelope
does not establish whether a ticket was issued or a consuming action committed.
The client MUST NOT automatically repeat assurance, create, or reset after such
an outcome, and it MUST NOT re-fetch or recover a temporary password. A new
explicit reset is a new credential and may supersede a reset whose outcome was
unknown.

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

The typed profile serves every other route, except for the encrypted backup
route's explicitly defined binary `200 OK` success. It serializes structured
results, carries correlation identifiers, and is the only profile permitted to
emit the approved session and cross-site request forgery cookies. The backup
route's failures remain typed-profile JSON errors with correlation identifiers.
Both profiles enforce their own bounds; neither can emit a body the other is
responsible for.

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
