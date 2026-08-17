# Web UI Pre-Operational Status Surface

This document defines the Web UI **[Client Module](../../glossary.md#applications-and-interfaces)** transport contract for the Milestone 1 pre-operational status surface. It owns the route, public results, rejection behavior, and compatibility policy for `GET /api/v1/status`. The [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md) owns trusted lifecycle classification and route availability; the [Security Model](../../security-model.md) owns the cross-cutting security profile.

This surface does not implement Init, Restore, or normal client functions. It
does not own the Application Database selection transport contract, which the
[Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
owns. It does not own the same Client Module's embedded Web UI asset delivery,
which the
[Embedded Asset Delivery Design](embedded-asset-delivery-design.md) owns, or the
Web UI application's presentation and client-side behavior, which the
[Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
owns.

## Ownership And Capability

The compiled-in Web UI Client Module provides one transport-only
**[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)** in
Milestone 1. It maps the Server-owned lifecycle status projection to the
contract below, and separately exposes the Application Database selection
capability defined by the
[Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md).
The Server runtime owns direct TLS, listener and route composition, and
lifecycle gating; the lifecycle boundary owns the typed status value. This
module declares no User Plane or Administration Plane in this foundation.

The **[Weavelit CLI](../../glossary.md#applications-and-interfaces)** has no
pre-operational status surface. A later decision is required before any Init,
Restore, database-selection, normal-plane, or other pre-operational route is
introduced.

## Route Contract

The sole route is:

```http
GET /api/v1/status
```

It accepts no `Accept` header or exactly `Accept: application/json`. Version 1
does not support another media type or content negotiation. A successful
response is `200 OK`, has `Content-Type: application/json; charset=utf-8`, and
has exactly this shape:

```json
{"lifecycle":"uninitialized","database_selected":false}
```

`lifecycle` is always the literal string `uninitialized`.
`database_selected` reports only whether an
**[Application Database](../../glossary.md#applications-and-interfaces)** has
been selected. It reflects the current lifecycle projection rather than a value
fixed at startup, so it becomes `true` within the same pre-operational session
once a selection succeeds through the
[Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md).

Every request sources that projection live from the Server-owned lifecycle
mutation authority, under the same exclusive permit that commits a selection.
The status route and the selection route share one authority, so a status read
issued after a successful selection cannot disagree with the projection that
selection returned. Startup classification chooses whether the route is mounted;
it never supplies the reported value. When the lifecycle authority cannot be
read, the route responds `503 Service Unavailable` with
`{"error":"service_unavailable"}` rather than reporting a stale or guessed
projection.

The response must not reveal database kind or configuration, deployment
identifiers, host information, filesystem information, diagnostics, health
detail, or another lifecycle detail.

## Lifecycle Availability

The runtime registers the route only after trusted lifecycle classification
reports an uninitialized deployment, with or without a selected Application
Database. It does not register the route for an Init-pending or Restore-pending
deployment, after the deployment is sealed, during normal operation, or after a
failed startup classification. Lifecycle availability is not inferred from a
client request.

## Rejections And Bounds

Every error response has `Content-Type: application/json; charset=utf-8` and
contains exactly `{"error":"<fixed-code>"}`. It contains no detail field or
diagnostic information.

| Condition | Response |
| --- | --- |
| Method other than `GET`, including an oversized syntactically valid non-`GET` method token | `405 Method Not Allowed`, `Allow: GET`, `{"error":"method_not_allowed"}` |
| Request body, malformed framing, target, or header, an oversized or malformed HTTP version, or unsupported `Accept` | `400 Bad Request`, `{"error":"bad_request"}` |
| Request target over 2 KiB | `414 URI Too Long`, `{"error":"uri_too_long"}` |
| Request headers over 8 KiB | `431 Request Header Fields Too Large`, `{"error":"request_header_fields_too_large"}` |
| Per-source rate exceeded | `429 Too Many Requests`, `{"error":"rate_limited"}` |
| Live lifecycle projection unreadable | `503 Service Unavailable`, `{"error":"service_unavailable"}` |
| Normal connection or handler capacity exhausted while the rejection lane is free | `503 Service Unavailable`, `{"error":"service_unavailable"}` |
| Normal connection or handler capacity exhausted while the rejection lane is occupied | Transport-level rejection with no HTTP response |
| Request read exceeds 5 seconds | `408 Request Timeout`, `{"error":"request_timeout"}` |
| Total request processing exceeds 10 seconds | `504 Gateway Timeout`, `{"error":"gateway_timeout"}` |

Rate admission consumes one per-source quota slot for every completed HTTP
request head that receives an HTTP response, including accepted heads and
completed heads classified as `400`, `405`, `414`, or `431`. A head is complete
only after the listener observes its terminating `\r\n\r\n`. When the source quota
is exhausted, `429` takes precedence over any other fixed HTTP response that a
completed head would otherwise receive. TLS handshake failures, capacity
rejections, incomplete EOF, and request-read timeouts do not consume quota.

Responses use connection-close framing without a `Content-Length` header and omit
the optional HTTP reason phrase. This preserves the documented status codes,
JSON bodies, media type, and `Allow: GET` behavior while keeping every fixed
status or error response on this route within the 128-byte limit. The same
connection-close, no-`Content-Length`, no-reason-phrase framing also applies to
the embedded asset responses served by the same module, which use the larger
per-profile bounds documented in the
[Embedded Asset Delivery Design](embedded-asset-delivery-design.md#size-bounds).
The direct TLS listener sends `close_notify` after each response; the response
write and TLS close use a bounded timeout.

The route accepts zero request-body bytes and does not interpret a request body,
decompress data, perform cryptographic work, start cancellation-sensitive
background work, or mutate state. The listener's separate bounded request-body
allowance applies only to `PUT`, which this route answers with `405` above, so
no body reaches this contract. The listener jointly permits at most 16 live
TLS connections or handshakes: at most 15 slots admit normal application
service work or handlers, and one separate slot is reserved for an overflow
connection. The overflow connection may complete direct TLS and emit only the
fixed `503` response above; it never dispatches an application route or state
work. Further overflow is transport-rejected without an HTTP response. The
request-read and total-processing bounds also apply to the rejection path, and
every slot releases deterministically when its connection ends. Each source may
make 20 route requests per minute with a burst of 12. One token bucket per
source covers the whole listener: the status route and every embedded asset
route draw from the same per-source budget, so a single browser page load
consumes one slot for the document, one for each asset, and one for the status
request. The burst therefore admits a full page load plus two immediate
reloads. This route's own success and error responses are fixed and no larger
than 128 bytes; the embedded asset routes sharing this budget return larger,
profile-bounded bodies as documented in the
[Embedded Asset Delivery Design](embedded-asset-delivery-design.md#size-bounds).

## Network And Browser Exposure

The deployment network boundary admits only IPv4 loopback `127.0.0.1/32` and
IPv6 loopback `::1/128` to this unauthenticated surface. The Server provides no
allowlist configuration channel. The response sends no CORS headers, supports no
credentials or cookies, provides no browser cross-origin interaction, and has no
CSRF flow.

The direct TLS listener must reject every TCP peer whose source address is not
exactly `127.0.0.1` or `::1` at transport admission, before capacity allocation,
TLS handshake, request parsing, or rate limiting.

## Compatibility

The success and error media types, field meanings, and fixed error codes are
stable in `/api/v1/`. Clients must ignore additive JSON fields, and additive
changes are permitted. Removing or renaming a field, changing a field meaning,
lifecycle availability, status behavior, or error semantics requires `/api/v2/`.

## Validation

Implementation must provide HTTP contract and direct-TLS process tests for both
`database_selected` values; every lifecycle availability boundary; accepted and
rejected `Accept` values; each listed rejection; the network, rate, concurrency,
and timeout limits; absence of CORS, cookies, normal routes, and a cleartext
listener; response-size bounds; and redaction. Implementation must also prove
the projection is live by observing, in one process, that a status read issued
after a successful Application Database selection reports
`database_selected: true`. The Server quality gate remains `make -C server
check`.

## Related Documents

- [Web UI Pre-Operational Database Selection Surface](pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Restore Surface](pre-operational-restore-design.md)
- [Embedded Asset Delivery Design](embedded-asset-delivery-design.md)
- [Web UI Application Design](../../clients/web-ui/web-ui-application-design.md)
- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Architecture Design](../../server/server-architecture-design.md)
- [Testing and Validation Policy](../../testing.md)