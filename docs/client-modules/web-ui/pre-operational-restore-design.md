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
identifier, or anything derived from either.

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

### Media Types And Negotiation

The request must carry exactly one `Content-Type: application/json` header. A
parameterized value, another media type, a missing header, or a duplicate header
is rejected. The request must carry either no `Accept` header or exactly one
`Accept: application/json`.

### Success

An accepted submission responds `202 Accepted` with
`Content-Type: application/json; charset=utf-8` and this typed envelope:

```json
{"result":{"restore_ticket":"<43-character token>"},"correlation_id":"<identifier>"}
```

`202` is the accurate status: the recovery key has been retained and a ticket
issued, but no Restore has run and no state has changed.

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
- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Testing and Validation Policy](../../testing.md)
- [Glossary](../../glossary.md)
