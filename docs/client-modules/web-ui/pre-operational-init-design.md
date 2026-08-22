# Web UI Pre-Operational Init Surface

This document defines the Web UI **[Client Module](../../glossary.md#applications-and-interfaces)**
transport contract for submitting **[Init](../../glossary.md#states-and-requests)**
before a deployment is initialized. It owns the two routes of the submission
protocol, the accepted request schema, media-type and negotiation handling,
body and collection bounds, the same-origin and cross-site request forgery
(CSRF) preconditions, the two success envelopes, and the complete rejection
contract for both routes.

The [Server Init Design](../../server/lifecycle/init/init-design.md) owns
recovery-key generation, checkpoint persistence, proof-of-possession
verification, Log Module assignment validation, and the atomic transition into
initialized application state. The
[Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns Init
eligibility and Application Database selection. The
[Security Model](../../security-model.md) owns the cross-cutting security
profile and protected-asset classification.

## Scope And Ownership

This surface owns only the transport contract for two routes. It does not own:

- recovery-key generation, checkpoint validation, proof-of-possession
  comparison, or persistence, which the
  [Server Init Design](../../server/lifecycle/init/init-design.md) owns;
- Init eligibility or **[Application Database](../../glossary.md#applications-and-interfaces)**
  selection, which the
  [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns;
- the read-only status contract, which the
  [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
  owns, including the listener-wide connection, rate, timeout, header, target,
  and response-size bounds that apply to every route on the same listener;
- Application Database selection transport, which the
  [Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
  owns; or
- the browser application's presentation and control affordances, which the
  [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
  owns.

## Implementation Status

The `weavelit-module-client` crate implements both routes, their request
schemas, and the complete rejection contract described below, with direct
tests for schema validation, header preconditions, proof-of-possession shape
checking, both success envelopes, and redaction of every submitted or
delivered secret. The Web UI Client Module declares this contract by supplying
an `InitCapability`, and the `weavelit-server` runtime composes and mounts it
the way it composes Restore: the recovery-key route is mounted whenever an
Application Database is selected, and the finalization route is mounted only
after the recovery-key response has actually been written, as the
[Server Init Design](../../server/lifecycle/init/init-design.md#recovery-key-delivery-and-finalization)
defines. Init is reachable and fully tested over the API this contract
describes.

The [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
Init workflow drives both requests through the browser: it presents the
first-launch choice between Init and Restore, the log assignment and first
Administrator steps, the one-time recovery-key display and acknowledgement
gate, and the review and finalization steps that submit these two requests.
That document owns the workflow's presentation, step sequence, and failure
handling; this contract remains authoritative only for the wire-level
request, response, and rejection shapes, and for the browser-side proof
derivation described below.

## Ownership And Capability

An Init-capable Client Module declares this contract by supplying an
`InitCapability` to `InitDeclaration`, which splits it into the two routes
below so a runtime can mount the finalization route only once the
recovery-key route has already responded. The module performs transport
validation only: it decides whether a request is well formed, same-origin,
and schema-valid, then delegates to the Server-owned Init contract and renders
exactly what that contract returns. It owns no lifecycle authority, no key
material, and no orchestration of its own.

## Two-Request Submission Protocol

Init is submitted in two requests:

```http
PUT /api/v1/init/recovery-key
PUT /api/v1/init
```

The first request carries the complete initialization request and receives
the one-time private recovery key and the delivery nonce that key must be
proved against. The second carries the same initialization request again,
together with the proof of possession computed from the delivered key, and
completes Init.

Splitting the submission is what makes the delivered key one-time without
making the deployment initialized before the requesting client has
demonstrably kept it: the first request creates only a non-operational
checkpoint, and the second is the only request that can replace it. Unlike the
Restore submission protocol, no bearer ticket binds the two requests; the
second request instead resubmits the full initialization request and presents
the proof of possession. `PUT` is used for both requests, because only `PUT`
may carry a body and both requests carry one.

## Recovery-Key Route Contract

```http
PUT /api/v1/init/recovery-key
```

### Request Schema

The request body must be exactly this JSON object:

```json
{"database":{"backend":"sqlite"},
 "administrator":{"username":"...","display_name":"...","password":"..."},
 "log_modules":[{"module":"...","name":"...","enabled":true,
                 "settings":[{"key":"...","value":"..."}],
                 "protected_settings":[{"key":"...","value":"..."}]}],
 "system_log":"...","audit_log":"..."}
```

`database.backend` confirms the client's view of the backend already selected
through `PUT /api/v1/application-database`; it carries no connection
configuration and never selects a database itself. `administrator.username`
and `administrator.password` are required; `administrator.display_name` is
optional and reported as absent when omitted. `log_modules` must contain
between 1 and 16 entries; each entry's `settings` accepts at most 64 members
and `protected_settings` accepts at most 16 members. `system_log` and
`audit_log` each name the configuration assigned to that log. This route
rejects `recovery_key_proof` as an unknown field, because the proof does not
exist until the delivered key is proved against.

Validation is strict and total: an unknown field, a duplicate key, a missing
member, a wrongly typed value, an oversized collection, the JSON array form,
content trailing the top-level value, and a body over 1 KiB are all request
errors. This transport decides only that the request is well formed; whether
the names are acceptable, the modules are compiled in, the assignments
resolve, and the confirmed backend is the one actually selected are decided by
the Server-owned Init contract.

### Media Types And Negotiation

The request must carry exactly one `Content-Type: application/json` header. A
parameterized value, another media type, a missing header, or a duplicate
header is rejected. The request must carry either no `Accept` header or
exactly one `Accept: application/json`.

### Success

An accepted submission responds `200 OK` with
`Content-Type: application/json; charset=utf-8`,
`Cache-Control: no-store`, and this typed envelope:

```json
{"result":{"recovery_key":"AGE-SECRET-KEY-1...","delivery_nonce":"<opaque-value>",
           "reconciliation_capability":"<opaque-value>"},
 "correlation_id":"<identifier>"}
```

`recovery_key` is delivered as one canonical age identity line and is returned
in this one response only; the Server never redisplays it. `delivery_nonce` is
the opaque value the client's proof of possession must be computed against.
`reconciliation_capability` is a separate high-entropy opaque value, unrelated
to the recovery key or the proof, that lets the browser later confirm whether
this exact submission is the one that completed, through the lifecycle
reconciliation route the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation)
defines. It is likewise returned in this one response only and is held only in
the requesting page's transient memory; it is never written to a URL, a
cookie, `localStorage`, or `sessionStorage`.

## Finalization Route Contract

```http
PUT /api/v1/init
```

### Request Schema

The request body is the same initialization request the recovery-key route
accepted, with one additional required member:

```json
{"database":{"backend":"sqlite"},
 "administrator":{...},
 "log_modules":[...],
 "system_log":"...","audit_log":"...",
 "recovery_key_proof":"<43-character token>"}
```

`recovery_key_proof` is required on this route and is the proof of possession
computed from the delivered private recovery key. It must be exactly 43
characters of unpadded URL-safe Base64, the fixed encoded length of an
untruncated HMAC-SHA-256 value. An absent or empty proof is its own rejection
category, distinct from a present but malformed or non-matching one. Whether a
well-shaped proof matches the checkpoint's expected value is decided by the
Server-owned Init contract, which holds the only value it could be compared
against; this transport checks shape only.

Every schema rule the recovery-key route enforces applies unchanged, and this
route additionally requires the `recovery_key_proof` member.

## Browser-Side Proof Derivation

The Web UI Client Module accepts `recovery_key_proof` as an opaque
43-character token and does not itself derive or verify it; the
[Server Init Design](../../server/lifecycle/init/init-design.md) computes the
expected value the checkpoint compares against. This section documents what
the [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
Init workflow sends and never sends, because the derivation happens entirely
in the browser between the two requests above.

The Init workflow decodes the delivered `recovery_key`'s Bech32 payload to
recover its 32-byte secret, then computes an `HMAC-SHA-256` over the raw bytes
of the delivered `delivery_nonce`, keyed by that secret, using only the
browser's native `crypto.subtle`. The workflow carries no JavaScript
cryptography dependency for this derivation; Bech32 decoding is implemented in
the application itself. The resulting signature, encoded as unpadded URL-safe
Base64, is the 43-character `recovery_key_proof` submitted on the
finalization request.

Only the derived proof is ever sent back to the Server: the finalization
request body never contains the `recovery_key` value itself. The delivered
private recovery key is held in browser memory only for the interval between
the two requests, and is never written to a URL, a cookie, `localStorage`, or
`sessionStorage`. It is dropped only on an outcome that is actually known: a
finalization the Server confirmed, or a rejection that permanently closes the
workflow. An attempt whose outcome was never reported keeps the key in that
same transient memory, because it may be the key this deployment was sealed
with. The
[Web UI Application Design](../../clients/web-ui/web-ui-application-design.md#init-workflow)
owns how that case is presented and settled.

### Success

A completed Init responds `200 OK` with
`Content-Type: application/json; charset=utf-8` and this typed envelope:

```json
{"result":{"lifecycle":"initialized"},"correlation_id":"<identifier>"}
```

The response reveals no administrator identity, password,
**[Log Module](../../glossary.md#applications-and-interfaces)** secret,
recovery key, or delivery nonce.

## Same-Origin And CSRF Preconditions

Both routes change state and are reachable from a browser, so both enforce the
same-origin precondition defined by the
[Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md#same-origin-and-csrf-preconditions),
unchanged: exactly one `Origin` header, exactly one `Host` header, exactly one
`X-Weavelit-CSRF: 1` header, an expected authority derived only from the
trusted listener socket address the Server actually bound, and never from a
certificate or a request header.

The precondition is evaluated before media-type validation and before any body
handling, so a cross-site request is denied without revealing negotiation or
schema detail.

## Credential Body Handling

Both Init bodies carry plaintext secret material: the initial administrator
password, protected Log Module settings, and the recovery-key proof. Both
therefore follow the shared secret request-body contract defined by the
[API Contract Design](../../server/api/api-contract-design.md#secret-request-body-handling)
without variation. The collected request buffer is taken into sole ownership
before it is read and is cleared when that ownership ends, on the accepted path
and on every rejection path alike.

The parser places every decoded protected Log Module setting value and
recovery-key proof in a clearing owner as it is read. A duplicate, unknown,
missing, or later trailing content rejection therefore drops a clearing owner
rather than a standalone plaintext allocation.

That contract's limits apply here unchanged: the clear is defense in depth
rather than a whole-process guarantee, because the transport layer's own read
buffers are outside this surface, and a buffer this surface cannot take sole
ownership of is cleared only in the copy it owns, never rejected.

## Rejections

Every error response has `Content-Type: application/json; charset=utf-8` and
contains exactly `{"error":"<fixed-code>"}`. The fixed body is the entire
payload: no response carries a detail, diagnostic, field path, or reason value
that could distinguish which specific check failed, and none carries a
password, a Log Module secret, a recovery key, a delivery nonce, or a proof.

| Condition | Response |
| --- | --- |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `{"error":"method_not_allowed"}` |
| Malformed, oversized, or schema-invalid body; wrong, missing, or duplicate `Content-Type`; unsupported or duplicate `Accept` | `400 Bad Request`, `{"error":"bad_request"}` |
| Finalization submitted with no proof of possession | `400 Bad Request`, `{"error":"recovery_key_confirmation_required"}` |
| Proof of possession that is malformed or does not match the checkpoint | `400 Bad Request`, `{"error":"recovery_key_confirmation_invalid"}` |
| Failed `Origin`, `Host`, or `X-Weavelit-CSRF` precondition | `403 Forbidden`, `{"error":"request_origin_denied"}` |
| Lifecycle state no longer permits this Init operation | `409 Conflict`, `{"error":"already_initialized"}` |
| Request validation, persistence, logging, or sealing failure | `500 Internal Server Error`, `{"error":"initialization_failed"}` |
| Backend, persistence, or integrity failure | `503 Service Unavailable`, `{"error":"service_unavailable"}` |

`recovery_key_confirmation_invalid` covers a malformed proof and a well-shaped
proof that does not match the checkpoint alike; those causes remain mutually
indistinguishable, so a rejection cannot report whether a guessed proof
partially matched.

One listener-wide answer means something different on the finalization route
than a rejection in the table above. The `504 Gateway Timeout`,
`{"error":"gateway_timeout"}` response defined by the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds)
is written when the listener stops waiting for the route, not by the route, and
the Server's finalization work continues past it. It therefore reports no
outcome at all rather than a failure: the deployment may still have been
initialized and sealed with the delivered key. A client must not treat it as a
rejection, must not discard the key it holds, and must establish the outcome
from a later observation before deciding anything. The
[Server Init Design](../../server/lifecycle/init/init-design.md#recovery-key-delivery-and-finalization)
owns the Server-side deadline observation that keeps this case rare, and its
stated residual.

That listener answer is not the only result reporting no outcome. A
finalization request whose transport fails after it was issued, and a
completion response whose body never reaches the client intact, tell a client
exactly as little: the request may have been delivered, and the commit chain it
started is not cancelled by either. The obligation above therefore applies to
every result a client could not read an answer from, whatever its local cause,
and applies to the delivered key most of all.

A rejection this surface did report is not such a result, however unspecific
that rejection is. A client that presents one fixed code for a failure carrying
no code of its own must therefore distinguish the two by whether a response
carried a code at all, not by the code it presents, so a determinate rejection
is never mistaken for an outcome that was never established.

The later observation is not this surface, which a sealed deployment stops
serving entirely. It is the submission-bound lifecycle reconciliation route,
`PUT /api/v1/lifecycle/reconciliation`, defined by the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation),
which a client calls with the `reconciliation_capability` this recovery-key
response delivered, held only in this page's transient memory. A `200`
`reconciliation_confirmed` result is the only outcome that proves this exact
finalization completed. A `404 Not Found` is not evidence of failure: a
capability that does not match the deployment's currently retained digest, a
capability for a deployment that never completed, and a capability this
client never actually held all answer identically, so a client cannot
distinguish the cause from this response alone. A `503 Service Unavailable`, a
transport failure, or an unreadable body are equally inconclusive. The
reconciliation route neither reads nor changes the generic session route,
`/api/v1/auth/session`; it answers from the live reconciliation digest alone,
so an attempt can be reconciled before, during, or without ever exercising
sign-in. A client must not discard a delivered key while an attempt is
unsettled, and must not write it anywhere to keep it.

Once an attempt has gone unsettled, two answers stop being determinate for
that client. `already_initialized`, defined in the table above, is answered
whenever the deployment record has left `Uninitialized`, which is exactly what
a finalization that committed leaves behind, and the `404 Not Found`,
`{"error":"not_found"}` answer a published normal operation gives on every
route it does not mount is what a sealed deployment answers here. Neither
proves the unsettled attempt committed,
because a lifecycle pending some other workflow answers the first and a Server
serving nothing at all answers the second, so a client must reconcile a retry
answered by either against the lifecycle reconciliation route rather than
believe it. A `reconciliation_confirmed` result settles the attempt as a
completed Init; no such result leaves it unsettled, never failed. This applies
only after an attempt has reported no outcome: a first submission answered by
either code was answered about itself and is presented as the rejection it is.
The Server's responses are unchanged by any of this; the reconciliation is
entirely a client obligation.

## Bounds And Exposure

Both routes accept at most 1 KiB of request body and stay within the
listener's default body bound; neither is given an admitted transport profile
of its own. `log_modules` accepts at most 16 entries, each with at most 64
non-secret settings and 16 protected settings.

Every listener-wide bound defined by the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds)
applies unchanged, including the loopback-only network boundary, connection
and handshake capacity, the shared per-source rate budget, request-target and
header-section limits, and connection-close response framing.

Both routes emit no cross-origin resource sharing (CORS) headers, set no
cookie, accept no credentials, and answer no preflight. Only the successful
recovery-key response carries the closed secret-disclosure cache effect and its
fixed `Cache-Control: no-store` header. Finalization results and every rejection
carry no cache-control effect; the Web UI still uses no-store request behavior
for every route.

## Compatibility

The routes, request schema, success field meanings, and fixed error codes are
stable in `/api/v1/`. Clients must ignore additive JSON response fields, and
additive response changes are permitted. Because the request schema rejects
unknown fields, adding a request member is a compatible server-side change
only when the previously accepted body remains valid. Removing or renaming a
field, changing a field meaning, changing the submission protocol's request
count or ordering, changing the same-origin or CSRF preconditions, or changing
error semantics requires `/api/v2/`.

## Validation

`weavelit-module-client` already provides contract tests for the accepted
request body and every listed schema rejection, including unknown fields,
duplicate keys, an oversized body, an oversized `log_modules`, `settings`, or
`protected_settings` collection, and the array form; accepted and rejected
`Content-Type` and `Accept` values on both routes; every same-origin, `Host`,
and CSRF rejection; an absent, empty, and malformed proof of possession; both
success envelopes; and the absence of any submitted secret or delivered key
from a rendered rejection, request, or debug form.

Implementation must also prove that each collected Init buffer is cleared when
its ownership ends, on the accepted path and on a parse rejection alike. The
proof must observe the buffer while it is still owned; reading released memory
is not a permitted test.

`weavelit-server`'s composition tests now prove the mounting behavior: that
the finalization route is reachable only after the recovery-key route has
actually written its response, that a proof mismatch and a lifecycle state
that no longer permits Init are correctly mapped, that neither route is
mounted before an Application Database is selected or after the deployment is
sealed, and that a stale surface still mounting both routes is rejected at
request time rather than by route absence.
`server/web-ui/browser-tests/init-first-launch.spec.ts` drives both requests
through the real release Server binary end to end, covering the mutually
exclusive first-launch choice, the shared Application Database selection, the
delivered key's copy and acknowledgement gate, the browser-derived proof of
possession, and the resulting sign-in. A second scenario in that file
intercepts exactly one finalization response as `504` and proves that the
delivered key survives an outcome that reported nothing and that the retry
completes Init with that same key, without a second key ever being prepared.
The Server quality gate remains `make -C server check`.

## Related Documents

- [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Restore Surface](pre-operational-restore-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Server Init Design](../../server/lifecycle/init/init-design.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Authentication Design](../../server/authentication/authentication-design.md)
- [Server API Contract](../../server/api/api-contract-design.md)
- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Testing and Validation Policy](../../testing.md)
- [Glossary](../../glossary.md)
