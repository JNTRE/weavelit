# Web UI Pre-Operational Database Selection Surface

This document defines the Web UI **[Client Module](../../glossary.md#applications-and-interfaces)**
transport contract for selecting the
**[Application Database](../../glossary.md#applications-and-interfaces)** before
**[Init](../../glossary.md#states-and-requests)** or
**[Restore](../../glossary.md#states-and-requests)**. It owns the route, the
accepted request schema, media-type and negotiation handling, body bounds, the
same-origin and cross-site request forgery (CSRF) preconditions, the success
projection, and the complete rejection contract for that route.

The [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns
trusted backend declaration, selection eligibility, persistence, and the typed
failure families this surface maps. The
[Security Model](../../security-model.md) owns the cross-cutting security
profile and protected-asset classification.

## Scope And Ownership

This surface owns only the transport contract for one route. It does not own:

- lifecycle classification, selection eligibility, or persistence, which the
  [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns;
- the read-only status contract, which the
  [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
  owns, including the listener-wide connection, rate, timeout, header, target,
  and response-size bounds that apply to every route on the same listener;
- embedded browser asset delivery, which the
  [Embedded Asset Delivery Design](embedded-asset-delivery-design.md) owns; or
- the browser application's presentation, control affordances, and usability,
  which the
  [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
  owns.

Init, Restore, and normal application functions remain absent from this surface.

## Ownership And Capability

The compiled-in Web UI Client Module declares an Application Database selection
capability on its
**[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)** in
addition to its status capability. The module performs transport validation
only: it decides whether a request is well formed, same-origin, and
schema-valid, then delegates the selection decision to the Server-owned
lifecycle contract and maps the typed result back to this contract.

The module is not the selection authority. It does not evaluate deployment
state, backend eligibility, replacement rules, or persistence outcomes, and it
never infers availability from a client request. The
**[Weavelit CLI](../../glossary.md#applications-and-interfaces)** declares no
database-selection capability, so no equivalent route exists on its surface.

## Route Contract

The sole route is:

```http
PUT /api/v1/database
```

`PUT` expresses the operation's idempotent intent: the request declares the
desired selected backend rather than appending a new record.

### Request Schema

The request body must be exactly this JSON object:

```json
{"backend":"sqlite","settings":{}}
```

Both members are required. `backend` accepts the single literal string
`sqlite`, which is the Milestone 1 MVP backend. `settings` accepts only an empty
JSON object, because the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
derives every SQLite artifact path itself and accepts no client-supplied path,
filename, or connection value.

Validation is strict and total. Insignificant whitespace and member ordering are
accepted. Every other deviation is a request error, including an unknown
top-level or `settings` field, a duplicate key, a missing member, a wrongly
typed value, an unknown or differently cased `backend` literal, a `settings`
value that is not an object, content trailing the top-level value, an empty
body, and a body over 1 KiB.

### Media Types And Negotiation

The request must carry exactly one `Content-Type: application/json` header. A
parameterized value such as `application/json; charset=utf-8`, another media
type, a missing header, or a duplicate header is rejected. The request must
carry either no `Accept` header or exactly one `Accept: application/json`.
Version 1 supports no other media type and no content negotiation.

### Same-Origin And CSRF Preconditions

This route changes state and is reachable from a browser, so it enforces a
same-origin precondition before any body handling.

The expected authority is derived only from the trusted listener socket address
the Server actually bound. It is never derived from a certificate subject
alternative name and never from a request header, so a client cannot influence
the value it is compared against.

The request must carry:

- exactly one `Origin` header;
- exactly one `Host` header; and
- exactly one `X-Weavelit-CSRF: 1` header.

`X-Weavelit-CSRF` is a non-simple header. A browser cannot send it cross-site
without a preflight, and this surface answers no preflight and emits no
cross-origin resource sharing (CORS) headers, so a cross-site request cannot
satisfy it.

Both authorities are normalized to an IP literal and an effective port, then
compared to the expected values. The `Origin` must use the `https` scheme.
Port 443 may be explicit or omitted; every other port must be explicit. An IPv6
literal must be bracketed on the wire. Comparison is on the parsed address
value, so equivalent IPv6 spellings match.

The precondition rejects a DNS name, an unbracketed IPv6 literal, userinfo, a
path, a query, a fragment, an opaque `null` origin, a non-`https` scheme, a
duplicate or missing header, and any authority mismatch between `Origin`,
`Host`, and the expected listener authority.

### Success

A successful selection responds `200 OK` with
`Content-Type: application/json; charset=utf-8` and exactly this body:

```json
{"lifecycle":"uninitialized","database_selected":true}
```

The response reuses the status surface's projection shape so a client observes
one consistent representation. It reveals no database kind, path, configuration,
deployment identifier, host information, filesystem information, diagnostic, or
other lifecycle detail. Selection does not initialize the deployment, so
`lifecycle` remains the literal string `uninitialized`.

## Rejections

Every error response has `Content-Type: application/json; charset=utf-8` and
contains exactly `{"error":"<fixed-code>"}`. The fixed body is the entire
payload: no response carries a detail, diagnostic, field-path, or reason value
that could distinguish which specific check failed.

| Condition | Response |
| --- | --- |
| Method other than `PUT` | `405 Method Not Allowed`, `Allow: PUT`, `{"error":"method_not_allowed"}` |
| Malformed, oversized, or schema-invalid body; wrong, missing, or duplicate `Content-Type`; unsupported or duplicate `Accept` | `400 Bad Request`, `{"error":"bad_request"}` |
| Failed `Origin`, `Host`, or `X-Weavelit-CSRF` precondition | `403 Forbidden`, `{"error":"request_origin_denied"}` |
| Lifecycle state no longer permits the requested selection | `409 Conflict`, `{"error":"database_selection_not_allowed"}` |
| Backend, integrity, persistence, or serialization failure prevented selection | `503 Service Unavailable`, `{"error":"service_unavailable"}` |

The same-origin and CSRF precondition is evaluated before media-type validation
and before any body handling, so a cross-site request is denied without
revealing negotiation or schema detail.

### Lifecycle Failure Mapping

The lifecycle contract returns a transport-neutral failure family. This surface
maps each family to exactly one outcome above:

| Lifecycle failure family | Response |
| --- | --- |
| Request invalid | `400 Bad Request`, `{"error":"bad_request"}` |
| Conflict | `409 Conflict`, `{"error":"database_selection_not_allowed"}` |
| Unavailable | `503 Service Unavailable`, `{"error":"service_unavailable"}` |

The mapping is total. Adding a failure family to the lifecycle contract is a
compile-time break here, so no failure can reach a client unmapped.

## Bounds And Exposure

The route accepts at most 1 KiB of request body. It performs no decompression
and no cryptographic work on the request, and it rejects an oversized body
before schema parsing.

Every listener-wide bound defined by the
[Web UI Pre-Operational Status Surface](pre-operational-status-design.md#rejections-and-bounds)
applies unchanged, including the loopback-only network boundary, connection and
handshake capacity, the shared per-source rate budget, request-read and
total-processing timeouts, request-target and header-section limits, and
connection-close response framing.

Every success and error body on this route is fixed and within the 128-byte JSON
response profile, so no new response profile is introduced. The Server core
returns a fixed JSON body only when that exact body is present in its response
allowlist and otherwise replaces it with the redacted response. Each body
defined here is therefore an allowlist entry, and adding a body to this contract
without adding the matching entry silently redacts a valid response.

The route emits no CORS headers, sets no cookie, accepts no credentials, and
answers no preflight.

## Compatibility

The route, request schema, accepted `backend` literals, success field meanings,
and fixed error codes are stable in `/api/v1/`. Clients must ignore additive
JSON response fields, and additive response changes are permitted. Because the
request schema rejects unknown fields, adding a request member, accepting a new
`backend` literal, or accepting a `settings` value is a compatible server-side
change only when the previously accepted body remains valid. Removing or
renaming a field, changing a field meaning, changing the same-origin or CSRF
preconditions, or changing error semantics requires `/api/v2/`.

## Validation

Implementation must provide table-driven contract tests for the accepted body
and every listed schema rejection, including duplicate keys, trailing content,
and the body limit at and above its bound; accepted and rejected `Content-Type`
and `Accept` values; every same-origin, `Host`, and CSRF rejection; accepted
authority normalization for explicit and omitted default ports across IPv4 and
IPv6 literals; each lifecycle failure family mapping; and the absence of CORS
and cookie headers on every response path.

Implementation must also prove that each fixed body defined here survives the
Server core's response allowlist rather than being redacted. The Server quality
gate remains `make -C server check`.

## Related Documents

- [Web UI Pre-Operational Status Surface](pre-operational-status-design.md)
- [Embedded Asset Delivery Design](embedded-asset-delivery-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Architecture Design](../../server/server-architecture-design.md)
- [Testing and Validation Policy](../../testing.md)
