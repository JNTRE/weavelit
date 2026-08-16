# Web UI Pre-Operational Restore Surface

This document defines the Web UI **[Client Module](../../glossary.md#applications-and-interfaces)**
transport contract for submitting a **[Restore](../../glossary.md#states-and-requests)**
before a deployment is initialized. It owns the two routes of the submission
protocol, the one-time submission ticket that binds them, the accepted request
schemas, media-type and negotiation handling, body bounds, the same-origin and
cross-site request forgery (CSRF) preconditions, the two success envelopes, and
the complete rejection contract for both routes.

The [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
owns backup validation, recovery-key handling, the compiled-in component
inventory a backup is judged against, checkpoint creation, and the orchestration
behind these routes. The
[Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns
Restore eligibility and database selection. The
[Security Model](../../security-model.md) owns the cross-cutting security
profile and protected-asset classification.

## Scope And Ownership

This surface owns only the transport contract for two routes. It does not own:

- Restore eligibility, validation, or persistence, which the
  [Server Restore Design](../../server/lifecycle/restore/restore-design.md) and
  the [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) own;
- the ticket store, its expiry schedule, or the lifecycle re-check that runs at
  request time, which the Server core owns;
- the read-only status contract, which the
  [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
  owns, including the listener-wide connection, rate, timeout, header, target,
  and response-size bounds that apply to every route on the same listener;
- Application Database selection, which the
  [Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
  owns; or
- the browser application's presentation and control affordances, which the
  [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
  owns.

## Ownership And Capability

The compiled-in Web UI Client Module is Restore-capable and declares a Restore
capability on its
**[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)**.
Both routes are mounted only while the deployment has already selected an
Application Database, so an unselected deployment serves neither route at all.

Mounting is not the authority. Every request re-checks Restore eligibility
against current trusted state, because the listener snapshots the whole serving
surface when it accepts a connection: a connection accepted while a Restore was
still eligible keeps a router that mounts both routes even after a checkpoint
exists.

The module performs transport validation only. It decides whether a request is
well formed, same-origin, schema-valid, and carries a well-shaped ticket, then
delegates to the Server-owned Restore contract and renders exactly what that
contract returns. The
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)** declares no
Restore capability, so no equivalent route exists on its surface.

## Two-Request Submission Protocol

A Restore is submitted in two requests:

```http
PUT /api/v1/restore
PUT /api/v1/restore/artifact
```

The first request carries the private recovery key alone and receives a
short-lived one-time ticket. The second carries the encrypted artifact as its
request body and presents that ticket in one exact custom header.

Splitting the submission is what keeps the recovery key off the large upload:
the key never travels with the artifact, so it is not resident for the duration
of a 256 MiB transfer, and the artifact is never admitted without a ticket the
Server itself issued. `PUT` expresses each request's idempotent intent.

### Submission Ticket

The ticket is an independent cryptographically random bearer value carrying 32
bytes of operating-system entropy, rendered as exactly 43 characters of
unpadded URL-safe Base64. It is never a correlation identifier, a session
identifier, or anything derived from either. From the moment it is minted, the
ticket value is held in a zeroizing owner through the Server's response
handoff, so no plaintext copy outlives the response that carries it.

The Server retains only a domain-separated SHA-256 digest of the ticket and
compares a submitted ticket against that digest in constant time. Its
properties are:

- **One outstanding submission.** A recovery-key submission made while another
  is outstanding is rejected rather than replacing it.
- **One-time.** Every claim consumes the retained submission, whether or not the
  claim succeeds. A replay, a concurrent claim, a wrong ticket, and an expired
  ticket all destroy the retained recovery key rather than leaving it available
  for another attempt.
- **Short-lived.** The ticket expires after the approved 120-second upload
  deadline, further capped at what remains of the total request deadline the
  first request started. An expired submission is destroyed on its own schedule
  rather than waiting for a second request that may never arrive.

The ticket is accepted only from the `X-Weavelit-Restore-Ticket` header. It is
never accepted from a URL, a query string, or a cookie, and it appears in
exactly one response: the one that issued it.

## Recovery-Key Route Contract

```http
PUT /api/v1/restore
```

### Request Schema

The request body must be exactly this JSON object:

```json
{"recovery_key":"AGE-SECRET-KEY-1..."}
```

`recovery_key` is required and is the only permitted member. Validation is
strict and total: an unknown field, a duplicate key, a missing member, a wrongly
typed value, content trailing the top-level value, an empty body, and a body
over 1 KiB are all request errors. The value is passed to the Server-owned
Restore contract without interpretation; whether it is one canonical age
identity line is decided there.

### Recovery-Key Body Handling

The recovery-key body is plaintext private-key material, so it follows the
shared secret request-body contract the
[API Contract Design](../../server/api/api-contract-design.md#secret-request-body-handling)
defines. The collected
request buffer is taken into sole ownership before it is read and is cleared
when that ownership ends. The clear runs on release rather than at one exit
point, so the accepted path, every rejection path, and any path added later
clear the same buffer. The parsed key itself is held only in a clearing wrapper
for the rest of the request.

If sole ownership of the collected buffer cannot be taken, the surface clears
the copy it does own and leaves the shared original to its owner. This handling
is defense in depth: the transport layer's own read buffers are outside this
surface, so what is guaranteed is that this surface retains no uncleared copy,
not that no copy exists anywhere in the process.

The artifact body is deliberately excluded. It is encrypted ciphertext that
discloses nothing on its own, and it is bounded by the artifact transport
profile rather than this 1 KiB bound.

### Media Types And Negotiation

The request must carry exactly one `Content-Type: application/json` header. A
parameterized value, another media type, a missing header, or a duplicate header
is rejected. The request must carry either no `Accept` header or exactly one
`Accept: application/json`.

### Success

An accepted submission responds `202 Accepted` with
`Content-Type: application/json; charset=utf-8` and this typed envelope:

```json
{"result":{"restore_ticket":"<43-character token>",
           "reconciliation_capability":"<opaque-value>"},
 "correlation_id":"<identifier>"}
```

`202` is the accurate status: the recovery key has been retained and a ticket
issued, but no Restore has run and no state has changed. `reconciliation_capability`
is a separate high-entropy opaque value, unrelated to the ticket, that lets the
browser later confirm whether this exact Restore is the one that completed,
through the lifecycle reconciliation route the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation)
defines. It is returned in this one response only and is held only in the
requesting page's transient memory; it is never written to a URL, a cookie,
`localStorage`, or `sessionStorage`.

## Artifact Route Contract

```http
PUT /api/v1/restore/artifact
```

### Request Body And Media Type

The request body is the encrypted backup artifact's exact bytes, with no
envelope, encoding, or multipart framing. The request must carry exactly one
unparameterized `Content-Type: application/octet-stream` header, because a
browser derives a file's media type from its name and the route accepts only
this one value. The request must carry either no `Accept` header or exactly one
`Accept: application/json`.

### Ticket Header

The request must carry exactly one `X-Weavelit-Restore-Ticket` header whose
value is a canonical opaque token. Like `X-Weavelit-CSRF`, it is a non-simple
header a browser cannot send cross-site without a preflight, and this surface
answers no preflight.

The header is checked for shape only at the transport boundary. Whether the
ticket is known, unexpired, and unclaimed is decided by the Server core, which
holds the only digest it could be compared against. A missing, repeated, or
malformed header is indistinguishable from an unknown, expired, or replayed
ticket.

### Success

A completed Restore responds `200 OK` with
`Content-Type: application/json; charset=utf-8` and this typed envelope:

```json
{"result":{"lifecycle":"initialized"},"correlation_id":"<identifier>"}
```

The response reveals no restored identity, backup content, deployment
identifier, database detail, host information, or filesystem information. The
correlation identifier is the same one the issuing response carried, so an
operator can tie a support conversation to one Restore without any protected
value.

## Same-Origin And CSRF Preconditions

Both routes change state and are reachable from a browser, so both enforce the
same-origin precondition defined by the
[Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md#same-origin-and-csrf-preconditions),
unchanged: exactly one `Origin` header, exactly one `Host` header, exactly one
`X-Weavelit-CSRF: 1` header, an expected authority derived only from the trusted
listener socket address the Server actually bound, and never from a certificate
or a request header.

The precondition is evaluated before media-type validation, before the ticket
header is examined, and before any body handling, so a cross-site request is
denied without revealing negotiation, ticket, or schema detail.

## Rejections

Every error response has `Content-Type: application/json; charset=utf-8` and
contains exactly `{"error":"<fixed-code>"}`. The fixed body is the entire
payload: no response carries a detail, diagnostic, field path, or reason value
that could distinguish which specific check failed, and none carries a recovery
key, a ticket, an artifact byte, or any backup content.

| Condition | Response |
| --- | --- |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `{"error":"method_not_allowed"}` |
| Malformed, oversized, or schema-invalid body; wrong, missing, or duplicate `Content-Type`; unsupported or duplicate `Accept` | `400 Bad Request`, `{"error":"bad_request"}` |
| Submitted value is not one canonical recovery key | `400 Bad Request`, `{"error":"recovery_key_invalid"}` |
| Malformed, unauthentic, altered, or otherwise invalid artifact, including a wrong recovery key | `400 Bad Request`, `{"error":"backup_invalid"}` |
| Unsupported backup format, mismatched source backend, or a component this build does not compile in | `400 Bad Request`, `{"error":"backup_incompatible"}` |
| Failed `Origin`, `Host`, or `X-Weavelit-CSRF` precondition | `403 Forbidden`, `{"error":"request_origin_denied"}` |
| Missing, malformed, unknown, expired, replayed, or concurrently claimed ticket | `403 Forbidden`, `{"error":"restore_ticket_invalid"}` |
| Lifecycle state no longer permits a Restore | `409 Conflict`, `{"error":"restore_not_allowed"}` |
| A Restore is already outstanding or in progress | `409 Conflict`, `{"error":"restore_pending"}` |
| Deadline, storage, or other internal Restore failure | `500 Internal Server Error`, `{"error":"restore_failed"}` |
| Backend, persistence, or integrity failure | `503 Service Unavailable`, `{"error":"service_unavailable"}` |

`backup_invalid` covers a wrong recovery key and an altered artifact alike;
those causes remain mutually indistinguishable, so a rejection cannot report
whether a guessed key partially matched.

### Results That Report No Outcome

`restore_failed` is a determinate answer: the Server reports it for a deadline,
storage, or other internal failure it observed itself, and a client may present
it as a failed Restore. A result that carries no answer at all is different.
The `504 Gateway Timeout`, `{"error":"gateway_timeout"}` response defined by the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds)
is written when the listener stops waiting for a route, not by the route. An
accepted artifact upload whose transport fails before its response arrives, and
a completion response whose body never reaches the client intact, leave a client
with as little. The Restore commit chain is not cancelled by any of them, so
this deployment may still be sealed and published afterwards.

A client must not present such a result as a failed Restore. It must establish
the outcome from a later observation before deciding anything, and must not
leave a retry offered against pre-operational routes a committed Restore no
longer serves. Because a transport or read failure carries no code of its own,
a client that presents one fixed code for it must distinguish it from a
`restore_failed` the Server actually sent by whether a response carried a code
at all, never by the code it presents.

An outcome is settled only by evidence, and this surface is not the evidence: a
sealed deployment stops serving it entirely. The observation that settles such
a result is the submission-bound lifecycle reconciliation route,
`PUT /api/v1/lifecycle/reconciliation`, defined by the
[API Contract Design](../../server/api/api-contract-design.md#lifecycle-reconciliation),
which a client calls with the `reconciliation_capability` the recovery-key
response delivered, held only in this page's transient memory. A `200`
`reconciliation_confirmed` result is the only outcome that proves this exact
Restore completed. A `404 Not Found` is not evidence of failure: a capability
that does not match the deployment's currently retained digest, a capability
for a deployment that never completed, and a capability this client never
actually held all answer identically, so a client cannot distinguish the cause
from this response alone. A `503 Service Unavailable`, a transport failure, or
an unreadable body are equally inconclusive. The reconciliation route neither
reads nor changes the generic session route, `/api/v1/auth/session`; it
answers from the live reconciliation digest alone, so an attempt can be
reconciled before, during, or without ever exercising sign-in. A client must
not discard a recovery key an unsettled attempt still needs, and must not
write it anywhere to keep it.

Once an attempt has gone unsettled, two answers stop being determinate for that
client. `restore_not_allowed`, defined in the table above, is answered whenever
the deployment record has left `Uninitialized`, which is exactly what a Restore
that committed leaves behind. The `404 Not Found`, `{"error":"not_found"}`
answer a Server gives on a route it does not mount is what a published normal
operation answers here, because it no longer mounts these routes at all.
Neither proves the unsettled attempt committed, because a lifecycle pending
some other workflow answers the first and a Server serving nothing at all
answers the second, so a client must reconcile a retry answered by either
against the lifecycle reconciliation route rather than believe it. A
`reconciliation_confirmed` result settles the attempt as a completed Restore;
no such result leaves it unsettled, never failed. This applies only after an
attempt has reported no outcome: a first submission answered by either code was
answered about itself, and a client presents it as the rejection this surface
defines. The Server's responses are unchanged by any of this; the
reconciliation is entirely a client obligation.

### Compiled-In Component Refusal

`backup_incompatible` is an operator-visible compatibility guarantee, not an
internal error. A backup names the components its source deployment used, and a
Restore succeeds only into a Server that can serve every one of them. The
[Server Restore Design](../../server/lifecycle/restore/restore-design.md#compiled-in-component-inventory)
owns the inventory this build actually compiles in and the exact rule applied.
A backup naming any other **[Client Module](../../glossary.md#applications-and-interfaces)**,
**[MFA Module](../../glossary.md#applications-and-interfaces)**,
**[Service Module](../../glossary.md#applications-and-interfaces)**,
**[Log Module](../../glossary.md#applications-and-interfaces)**, or named
operation is refused with `backup_incompatible` before any state changes, rather
than restored into a deployment whose Groups, factors, and connections would
point at components that could never load.

## Bounds And Exposure

The recovery-key route accepts at most 1 KiB of request body and stays within
the listener's default body bound. It is never given the artifact route's
admitted transport profile.

The artifact route is admitted under the approved Restore transport profile
defined by the
[Security Model](../../security-model.md#backup-input-security-profile): a
256 MiB maximum encrypted artifact, a 120-second upload deadline, and a
300-second total request deadline. Both deadlines are capped at what remains of
the budget the recovery-key submission started, so a second request cannot
restart the clock. At most one Restore may be admitted at a time; the admission
permit is the same single-permit mutation lane the approved concurrency bound
defines.

The artifact bound and its read budget are granted only to the mounted artifact
route, paired with it at mount time, so no other route can inherit them.

Every listener-wide bound defined by the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds)
applies unchanged, including the loopback-only network boundary, connection and
handshake capacity, the shared per-source rate budget, request-target and
header-section limits, and connection-close response framing.

Both routes emit no cross-origin resource sharing (CORS) headers, set no cookie,
accept no credentials, and answer no preflight. No response on either route is
cached: neither carries a recovery key or an artifact byte, and the one response
that carries a ticket carries it once.

## Compatibility

The routes, request schemas, ticket header name, success field meanings, and
fixed error codes are stable in `/api/v1/`. Clients must ignore additive JSON
response fields, and additive response changes are permitted. Because the
recovery-key request schema rejects unknown fields, adding a request member is a
compatible server-side change only when the previously accepted body remains
valid. Removing or renaming a field, changing a field meaning, changing the
submission protocol's request count or ordering, changing the same-origin, CSRF,
or ticket preconditions, or changing error semantics requires `/api/v2/`.

## Validation

Implementation must provide contract tests for the accepted recovery-key body
and every listed schema rejection, including duplicate keys, trailing content,
and the body limit at and above its bound; accepted and rejected `Content-Type`
and `Accept` values on both routes; every same-origin, `Host`, and CSRF
rejection; every ticket rejection, including a missing, repeated, malformed,
unknown, expired, replayed, and concurrently claimed ticket, together with proof
that a failed claim destroys the retained submission; each Restore failure
family mapping; and the absence of CORS and cookie headers on every response
path.

Implementation must also prove that a stale surface still mounting both routes
is rejected at request time by the lifecycle re-check rather than by route
absence, that neither route is mounted before an Application Database is
selected or after the deployment is sealed, and that no rejection body discloses
a ticket, a recovery key, or backup content.

Implementation must further prove that the collected recovery-key buffer is
cleared when its ownership ends, on the accepted path and on a parse rejection
alike, and that both route outcomes run through that same clearing owner. The
proof must observe the buffer while it is still owned; reading released memory
is not a permitted test.

Web UI end-to-end tests drive both requests through the real release Server
binary over its direct-TLS listener. The Server quality gate remains
`make -C server check`.

## Related Documents

- [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
- [Embedded Asset Delivery Design](embedded-asset-delivery-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Authentication Design](../../server/authentication/authentication-design.md)
- [Server API Contract](../../server/api/api-contract-design.md)
- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Testing and Validation Policy](../../testing.md)
- [Glossary](../../glossary.md)
