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
- `/api/v1/restore` and `/api/v1/restore/artifact` are the two requests of the
  Restore submission protocol. Their behavior is fixed by the
  [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md).
- Pre-operational lifecycle contracts are mounted only while their declaring
  capability is eligible.
- `/api/v1/auth/` carries authentication bootstrap. These routes are neither
  User Plane nor Administration Plane, because a principal does not yet exist
  when they are invoked.
- `/api/v1/user/` and `/api/v1/administration/` carry the two normal planes.

### Client Module Identity

Because Client Modules share routes, a request does not name its Client Module
in its path. The Server determines the Client Module from the authenticated
session, which records the Client Module the session was established for. A
request to create a session declares its Client Module explicitly.

This preserves per-Client-Module Group grants. Grants apply only to
authenticated requests, and pre-operational contracts run before any Human User
exists, so no grant applies to them.

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
so a route cannot emit a partial envelope that still parses as valid JSON. No
route emits a session or cross-site request forgery cookie yet.

## Pagination

A collection response is cursor-paginated. A request MAY supply `limit` and
`cursor`. `limit` defaults to 50 and MUST NOT exceed 100. A cursor is opaque and
carries no client-interpretable meaning. A collection result contains `items`
and a nullable `next_cursor`.

## Idempotency

Version 1 defines no global idempotency-key store. Existing contracts do not
require one: Application Database selection is naturally idempotent, Restore is
deliberately non-retryable once its checkpoint exists, and each authentication
attempt is a distinct event.

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
- [Server Lifecycle Design](../lifecycle/lifecycle-design.md)
- [Server Restore Design](../lifecycle/restore/restore-design.md)
- [Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
- [Web UI Pre-Operational Database Selection Surface](../../client-modules/web-ui/pre-operational-database-selection-design.md)
- [Web UI Pre-Operational Restore Surface](../../client-modules/web-ui/pre-operational-restore-design.md)
- [Glossary](../../glossary.md)
