# Server Architecture Design

This document records shared implementation-architecture decisions for the
**[Weavelit Server](../glossary.md#applications-and-interfaces)**. It owns
workspace-wide Rust crate structure, composition, and lifecycle rules that
apply across Server components. Feature-specific design remains in its owning
Server documentation boundary.

## Scope And Ownership

This document records decisions that affect more than one
**[Weavelit Server](../glossary.md#applications-and-interfaces)** component,
such as Rust workspace conventions, crate naming, compiled-in component
composition, and shared lifecycle boundaries. It does not replace the detailed
contract, storage, authentication, authorization, logging, or provider designs
owned by their respective documents.

A component-specific document links here when it applies a shared Server rule.
A shared rule is recorded here only when it remains useful outside the component
where the decision first arose.

## Rust Crate Naming

Server core, workflow, and infrastructure Rust crates use this package-name
convention:

```text
weavelit-server-<component>[-<specific-component>]
```

Compiled-in Module crates use this package-name convention:

```text
weavelit-module-<module-type>-<implementation>
```

`<module-type>` is `client`, `log`, `mfa`, or `service` and reflects the
canonical Module category. `<implementation>` identifies the client surface,
log destination, MFA method, or external service. For example:

```text
weavelit-module-client
weavelit-module-client-cli
weavelit-module-client-webui
weavelit-module-mfa-totp
weavelit-module-service-zendesk
```

Source directories group crates by ownership under `server/crates/core/`,
`server/crates/database/`, and `server/crates/modules/`. A grouping directory is
not a Cargo package and contains no `Cargo.toml`; each package lives in a child
directory whose name matches its Cargo package name. The workspace manifest
lists each supported compiled-in crate explicitly rather than discovering
packages through a broad directory glob.

`<component>` names the Server concern. The optional
`<specific-component>` names a concrete backend, destination, provider, or
other implementation when applicable. Names use Cargo package spelling with
hyphens; Rust imports use underscores.

Create a base crate only when it owns meaningful shared code or a shared
contract. An implementation crate may stand alone until a shared crate is
justified. A base Module crate omits `-<implementation>`, as
`weavelit-module-client` does for the shared Client Module contract described in
the [Server API Contract](api/api-contract-design.md). The naming convention
does not classify a component as a runtime module.

For example, the **[Application Database](../glossary.md#applications-and-interfaces)**
crates are:

```text
weavelit-server-database
weavelit-server-database-sqlite
```

The first crate owns the shared Application Database contract; the second owns
the SQLite implementation. This convention also permits a future dedicated
**[Log Module](../glossary.md#applications-and-interfaces)** implementation crate
such as `weavelit-module-log-sqlite`, without requiring a shared Log Module
crate before it has meaningful shared code or a shared contract.

The pre-operational Server crates are:

```text
weavelit-server-lifecycle
weavelit-server-init
weavelit-server-restore
```

The Server-owned logging producer crates are:

```text
weavelit-server-log-authority
weavelit-server-observability
```

The Server-owned authentication crate is:

```text
weavelit-server-authentication
```

`weavelit-server-authentication` owns the local password authentication core:
the current Argon2id profile, the closed allowlist of profiles a stored
verifier may be attempted against, the equal-work password decision, and
generation and hashing of session and cross-site request forgery bearer values.
It takes no workspace path dependency, so the transport, the listener, the
Application Database, and every
**[Client Module](../glossary.md#applications-and-interfaces)** are outside its
reach. A caller supplies the stored credential as an inbound value and persists
the replacement verifier and token digests the crate returns; the crate itself
neither reads nor writes storage and issues no cookie. The
[Authentication Design](authentication/authentication-design.md) owns the
profile, the allowlist policy, and the session representation it implements.

The shared Log Module contract is `weavelit-server-log`. It owns the Server
core's typed record and dispatch boundary, not log-record construction or a
destination implementation. `weavelit-module-log-sqlite` and a future
`weavelit-module-log-mysql` may implement that contract while retaining their
own persistence and delivery behavior.

`weavelit-server-observability` is Server Observability: the only producer of
complete, pre-redacted System Log records, including the Init and Restore
completion results. It selects classifications, bounds and redacts every field,
and builds a record together with the completion obligation persisted alongside
it, so a post-commit record cannot drift from committed state. It owns no
delivery, destination configuration, workflow orchestration, or Application
Database access. A workflow crate asks Observability for a prepared record
rather than constructing one.

`weavelit-server-log-authority` carries the capability that separates
Server-owned logging authority from an ordinary Log Module. Rust has no
cross-crate friend visibility, so the log contract cannot make its
authority-minting constructors reachable from Audit and Observability while
keeping them unreachable from a module. Holding a `ServerLogAuthority` is that
distinction, and obtaining one requires an explicit dependency edge that is
visible in a manifest and reviewable on its own. The log contract keeps its
original private constructors, does not reexport the capability, and its
compile fixtures prove that an external consumer can register a module but
cannot mint an issuer, trusted context, acknowledgement, dispatch, or the
capability itself.

`weavelit-server-lifecycle` is the internal base crate for lifecycle behavior
shared by **[Init](../glossary.md#states-and-requests)** and
**[Restore](../glossary.md#states-and-requests)**. The two workflow crates own
meaningfully different application-state transitions, so neither is an
implementation detail of the other.

## Pre-Operational Crate Boundaries

`weavelit-server-lifecycle` owns the shared trusted mechanisms required before
the **[Weavelit Server](../glossary.md#applications-and-interfaces)** can enter
normal operation: deployment-record and database-locator types and persistence,
startup classification, deployment-identifier binding, Application Database
selection orchestration, mutation serialization, lifecycle eligibility, and
fail-closed retained-state interruption classification. The runtime supplies
its compiled-in Application Database backend catalog and uses the lifecycle
result to choose which routes may exist. The lifecycle crate does not create
new application state, interpret backup contents, handle a private recovery
key, reconcile or seal retained partial state, or implement client presentation.

The initial delivered lifecycle contract depends only on
`weavelit-server-database`. It reuses that crate's deployment identifier and
Application Database trait while defining lifecycle record and locator values,
canonical backend and field identifiers, bounded scalar connection values,
trusted secret classifications, capability classifications, and payload-free
errors. `BackendCatalog` validates runtime registrations and submitted fields
before invoking an `ApplicationDatabaseFactory`. The factory receives a trusted
Server-derived local context separately from canonically ordered validated
settings and returns only the backend-neutral Application Database contract.
This initial boundary contains no persistence, serialization, cryptography,
SQLite implementation, Client Module, or runtime-composition dependency.

`weavelit-server-init` owns only the new-state workflow. It uses the lifecycle
crate to select and reopen the Application Database and to validate and advance
trusted lifecycle state. It owns initialization requests, first-user and
Administrators Group creation, initial Log Module configuration and assignment,
recovery-key generation and delivery, proof verification, the atomic creation of
new application state, and the required process-level Init-result durable
acknowledgement through the committed System Log assignment. Its detailed workflow is defined in the
[Server Init Design](lifecycle/init/init-design.md).

`weavelit-server-restore` owns only the existing-state workflow. It uses the
lifecycle crate to select and reopen an eligible Application Database and to
validate and advance trusted lifecycle state. It owns bounded encrypted backup
staging, backup and recovery-key validation, authenticated decryption, format
and compatibility validation, restored-session invalidation, protected-secret
re-encryption, atomic restoration, and the required process-level Restore-result
durable acknowledgement through the restored System Log assignment. It never exposes the private
recovery key or decrypted backup contents outside its Server-owned boundary.

The Init and Restore crates depend on the lifecycle and Application Database
contracts but do not depend on each other. Each mutating workflow entry point
calls the lifecycle authority itself before reading secrets or backup content
or causing side effects; a prior runtime or routing check is not sufficient.
This dependency direction keeps lifecycle enforcement consistent without
allowing either workflow to invoke or re-enable the other.

The `weavelit-server` runtime composes all three crates and exposes Init-capable
or Restore-capable **[Client Module](../glossary.md#applications-and-interfaces)**
routes only when the trusted lifecycle state permits them. All three crates
remain compiled into the Server after the deployment is sealed. "Unavailable"
or "disabled" means that the runtime exposes no corresponding routes and the
workflow entry points independently reject direct invocation; it does not mean
that Rust crates are dynamically unloaded.

## Compiled-In Component Boundaries

The **[Weavelit Server](../glossary.md#applications-and-interfaces)** composes
supported **[Application Database](../glossary.md#applications-and-interfaces)**
backends, pre-operational components, and runtime modules as compiled-in Rust
crates. The runtime owns composition; `weavelit-server-lifecycle` owns shared
pre-operational lifecycle behavior; and component crates own their
implementation-specific behavior behind their documented boundaries. The
runtime supplies backend and module catalogs, while each implementation owns
validation of its connection and storage settings.

A shared Server crate boundary must not erase the distinction between product
concepts. In particular, an Application Database backend is not a runtime
module. Application Database persistence remains separate from every
**[Log Module](../glossary.md#applications-and-interfaces)** destination even
when their implementations use the same technology. They may use the same
approved workspace dependency without sharing persistence behavior.

## HTTPS Runtime Composition

The `weavelit-server` runtime owns the sole direct-TLS listener, lifecycle
gating, and route composition. Its Milestone 1 status surface uses Axum routing
over Hyper and Tokio with Rustls. Rustls uses the approved AWS-LC cryptographic
provider and permits TLS 1.2 and TLS 1.3. The runtime does not create a second
listener or a cleartext fallback.

The compiled-in `weavelit-module-client` crate owns translation of the
pre-operational status request and Application Database selection request and
response contracts, their canonical route paths, and both the pre-operational
and operational capability declarations a Client Module returns. The
pre-operational declaration carries the status, Application Database selection,
and asset capabilities; the operational declaration carries only client asset
delivery and cannot express either pre-operational route, so a sealed
deployment's surface has no way to mount them. The compiled-in
`weavelit-module-client-webui` crate owns only what is browser-specific: the
capabilities the Web UI declares and delivery of its compile-time embedded
browser asset allowlist. The runtime mounts the declared surface
only when the Server-owned lifecycle gate permits it and retains ownership of
direct TLS, listener composition, raw request parsing, resource limits, and
lifecycle classification. The module cannot independently compose a route or
listener, and an undeclared capability is absent rather than present and denied.
The [Web UI Pre-Operational Status Surface](../client-modules/web-ui/pre-operational-status-design.md)
defines the status contract and resource limits; the
[Web UI Pre-Operational Database Selection Surface](../client-modules/web-ui/pre-operational-database-selection-design.md)
defines the selection contract; the
[Embedded Asset Delivery Design](../client-modules/web-ui/embedded-asset-delivery-design.md)
defines the asset allowlist, media types, security headers, and body bounds.

### Serving-Mode Switch

The listener serves exactly one of three modes: the pre-operational surface, a
fail-closed surface with no functional route, or the sealed deployment's
operational surface. Every mode is composed over the same fixed not-found
fallback, so a mode serves the routes its Client Module declaration supplied and
nothing else. A classified startup selects the initial mode: the two
uninitialized classifications serve the pre-operational surface, a retained
pending classification serves the fail-closed surface, and a sealed deployment
serves the operational surface.

The runtime publishes the current mode through one watch channel. A mode carries
its router and its transport registrations as one value, so the two are composed,
published, and snapshotted together and can never describe different route sets.
The listener snapshots that whole value when it accepts a connection and before
it spawns the connection task, so an in-flight connection continues serving the
surface it snapshotted and only a newly accepted connection observes a newer
mode. The publisher is a separate named capability that a workflow holds to move
a running listener from its pre-operational surface to fail-closed and then to
the operational surface without a restart. The publisher carries no lifecycle
authority: a caller must already have completed the trusted transition it
publishes.

The fail-closed mode is composed with no transport registration at all, so no
request it serves can reach a non-default transport profile. The pre-operational
and operational modes each mount their routes together with the registrations
those routes require.

### Operational Composer

One operational composer owns the whole operational surface. It accepts the
Application Database handle a sealed workflow hands over, mounts the Client
Module operational declaration over the shared not-found fallback, and attaches
every operational transport registration to that same mounted value. The
publisher accepts only what the composer produced, so an operational route
cannot be published as a bare router that has shed the ordered admission policy
the transport chain enforces. A registration-less operational surface is not
expressible rather than discouraged by convention.

Both routes into normal operation compose through that one composer: a sealed
startup and a completed in-process Restore each hand it the database they hold
open and publish the surface it returns. The two paths therefore cannot drift in
what they mount, in what registrations they carry, or in which database handle
their routes read.

The runtime composes every mounted pre-operational route over one shared
lifecycle authority. Startup constructs a single workflow arbiter over the
opened lifecycle store, together with the shared backend catalog and trusted
backend context, and hands each route a reference to that same instance. The
module holds no lifecycle state: the status route calls a Server-supplied source
that reads the projection live on every request, and the selection route
delegates its decision to a Server-supplied commit hook that returns the
projection observed under the same exclusive mutation permit that committed the
selection. A status read therefore cannot report a value captured at startup or
disagree with a completed selection. A future Init workflow must reuse this same
arbiter; composing a second one would defeat serialization between selection and
Init. The expected same-origin authority passed to the selection route is the
socket address the listener actually bound, never a request header or a
certificate subject alternative name.

### Restore Orchestration Composition

The `weavelit-server` runtime owns the only composition that joins the Restore
validation crate to the lifecycle typestate chain. Neither crate depends on the
other, so the ordering that makes a Restore safe is a runtime responsibility
rather than a property either crate can enforce alone. The orchestration takes
already-received bytes instead of a request, so the transport that delivers a
backup is composed over it without being able to change that ordering.

The orchestration shares the startup composition's workflow arbiter and its
single-permit mutation lane, so a Restore serializes against pre-operational
Application Database selection instead of racing it. It acquires that lane
without waiting: a Restore that queued would hold its artifact and recovery key
resident for the whole wait, and the Restore contract already admits one
operation at a time.

The whole authorize-through-seal chain runs in one blocking task. That keeps the
exclusive workflow permit, the checkpoint, the durable acknowledgement, and the
seal on a single thread with no cancellation point between them, so no caller
timeout can abandon a deployment mid-replacement. The runtime constructs the
Server Log Authority and Server Observability inside this composition and
retains the authority privately, so no other caller can mint a trusted record
issuer or a trusted Log Module context.

A sealed startup uses that same arbiter to load the deployment's application
state. The load runs under the exclusive mutation permit and independently
re-reads the deployment record and re-inspects the database exactly as sealing
does, because startup classification is a routing control rather than the
authority. A record and database that no longer agree, or that are bound to
another deployment, fail startup closed rather than producing a surface to
serve. Sealing returns the loaded state together with the database the workflow
held open, and the runtime retains both for the process lifetime.

Operational routes read through that same handed-over handle rather than
reopening the target, so a running deployment never holds two open handles to
one Application Database file. The handle is shared behind an exclusive lane, so
concurrent operational requests serialize on it exactly as lifecycle mutations
serialize on the workflow arbiter's lane. A completed in-process Restore hands
over the handle it committed through in the same way, so activation without a
restart does not reopen the database it just replaced.

The implementation selects minimal features and exact crates.io versions for
Axum, Hyper, Tokio, and their required adapters under the dependency policy
below. Each selected package must be maintained and advisory-reviewed before it
is added. A package upgrade is a deliberate dependency change that repeats the
version, change, advisory, and validation review; no future version is approved
by this architecture decision.

### Signalled Shutdown

The listener stops on a trigger the process supplies rather than on a signal it
installs itself, so deciding what asks the Server to stop stays process policy.
The process treats `SIGTERM`, which a service supervisor sends, and `SIGINT`,
which an interactive operator sends, as the same request to stop. The trigger is
registered before anything is served, so a stop that arrives during the first
request is still delivered. Because the listener takes the trigger as a value, a
test drives the identical shutdown path without raising a real signal.

A signalled shutdown runs in a fixed order:

1. Accepting stops. A shutdown already signalled always wins against a
   connection arriving at the same moment, so no connection is admitted after
   the request to stop.
2. The bound socket is released immediately, before draining begins, so the
   listener address is free for the next Server generation rather than held for
   the length of the drain.
3. Already-accepted connections keep being served to completion, response write
   included. Each holds the serving-mode snapshot it took when it was accepted,
   so no republished mode changes what it serves and no client is left owed a
   response the Server had already begun.
4. The Application Database is closed once draining ends.

Each stage is bounded separately, because the two fail differently: a request
that will not finish must not consume the allowance the database close needs.
Draining is allowed 25 seconds and the close is allowed 5 seconds, so a whole
shutdown is bounded at 30 seconds. The drain budget deliberately exceeds the
longest a single connection may occupy the listener, which is the TLS handshake,
request-read, and processing budgets in sequence, so a request admitted just
before the signal can still finish inside it; that relationship is asserted at
compile time rather than restated as a convention. Whatever the drain does not
finish is terminated before the close begins, so a request that will not end
cannot delay the database close behind it.

The close runs through one process-wide owner of whichever Application Database
is serving. A deployment becomes operational either from a sealed startup or
from an in-process Restore, and each keeps its own composition afterwards, so
shutdown cannot close the database by asking either path. Composing an
operational surface registers its database with that owner instead, which is the
one place both paths pass through, so what shutdown closes does not depend on
how the deployment became operational. The owner takes the handle out of every
clone at once, so the close happens exactly once however many times shutdown is
requested, and every later application operation is refused rather than racing a
closing backend. A duplicate stop request therefore reports the same clean
result rather than a second, different one. A lane left unusable by a panicking
operation is still closed exactly once, but the shutdown is reported as failed
however cleanly the backend closed, because the operation that poisoned the lane
has an untrusted outcome.

A shutdown that completes both stages inside their budgets exits with status
`0` and no terminating signal. A drain that does not finish, or a database that
does not close cleanly, is reported as `shutdown_incomplete` and exits with
status `1`; it is an unclean stop rather than a startup failure, and it is never
reported as a clean one. The exit status is returned rather than set by an
immediate process exit, so an orderly shutdown still unwinds normally and every
retained value, including the process-lifetime state-root lock, is released by
its own destructor.

A host supervisor must allow more than the Server's whole 30-second budget
before it kills the process, or it would kill a shutdown that is still inside
its own budget. A packaged service unit must therefore stop the Server with
`SIGTERM` and set a stop timeout greater than 30 seconds; 40 seconds is the
recommended value.

### Bounded Request Reading

The runtime reads a request head, admits it, and only then reads a body. The head
read applies the existing 2 KiB request-target, 8 KiB raw-header, and aggregate
head bounds. A body is read only when the head declares one and only after the
request has been admitted.

#### Route-Registered Transport Profiles

A transport profile supplies the maximum body size a request may declare and the
budget its body read and processing receive. Every request starts on the default
profile, which preserves the listener-wide behavior: at most 1 KiB of body, only
for `PUT`, one 5-second budget shared by the head and the body, and a 10-second
total processing budget measured from the start of the request.

A route earns a different profile only by being registered, and a registration
reaches the listener only bundled with the router mount that serves the same
route. A registration therefore cannot describe a route the published surface did
not mount, and a mounted route without a registration stays on the default
profile.

Classification matches the exact canonical request target and the exact method.
A query string, an absolute-form request target, a percent-encoded separator or
segment, a dot segment, a trailing slash, a prefix, a longer target, and any
other method all fail to match and receive the default profile. A registered
profile is therefore never reachable by rewriting a request target.

#### Admission Ordering

The runtime performs these steps in this order, and each step consumes the value
the previous step produced, so a different order does not compile:

1. Read the request head within its own absolute 5-second deadline.
2. Admit the head against the per-source rate limit.
3. Classify the request against the published surface's registrations.
4. Apply the classified profile's framing checks.
5. Run the registration's pre-body validation, if it declared any.
6. Acquire the registered route's admission permit.
7. Allocate the declared body fallibly, then read it.

The head's deadline is absolute in every case and is never lengthened by a route,
a registration, or an admitted body, so a slow request head still fails inside
the 5-second read budget. Only an admitted body may receive a longer body-read
and processing budget, measured from the moment admission completed. The wait for
an admission permit stays inside the connection's own processing budget and
answers with the fixed `504` response when it expires.

A registered route may bound how many bodies it admits at once. The permit is
acquired before any body memory is reserved and is handed to the route through
the request, so downstream work never acquires it again. Concurrent large-body
memory is therefore bounded by the registered permit count rather than by the
number of accepted connections. The allocation itself is fallible: a reservation
the host cannot satisfy answers with the fixed `503` service-unavailable response
rather than aborting the process.

#### Framing Rules

On the default profile only `PUT` may carry a body. A request that carries one
must declare exactly one canonical decimal `Content-Length` within the classified
profile's maximum. This body allowance is separate from the head bounds and never
relaxes them. The runtime rejects chunked transfer encoding, any `Expect` header,
a duplicated or conflicting `Content-Length`, a non-numeric, signed, or
non-canonical length, a declared length over the classified maximum, a stream
that ends before the declared length, and bytes beyond the declared length. Every
other method must declare no body at all, so a `GET` carrying body framing is
still rejected. Each rejection uses the fixed `400` bad-request response.

Per-source rate admission still keys on a completed request head, so a head read
that times out is a request timeout rather than a consumed quota slot.

A complete request keeps its method and is dispatched, so the mounted route
decides whether the method is permitted and which method it advertises. Only a
request line whose oversized method token never yields a bounded target is
classified before dispatch; that path has no route context and answers with the
fixed `405` and `Allow: GET`.

### Allowed-Method Representation

A bounded response carries an optional allowed method drawn from a closed
`AllowedMethod` set of `GET` and `PUT`. The response writer emits the matching
fixed `Allow` header line, or none when the response has no allowed method. A
module therefore selects one of these fixed values and can never supply header
text of its own.

The media type, security headers, and maximum body size continue to derive from
the response profile alone. They are never taken from a module, request, file
extension, or body, and the allowed method does not influence any of them.

## Rust Workspace Dependency Policy

`server/Cargo.toml` is the Server Rust workspace manifest and the authority for
workspace-wide dependency governance. This policy applies to every direct
production dependency in that workspace.

### Approved Production Dependencies

This document is the stable record of approved direct production dependencies
for the Server Rust workspace. It does not list transitive dependencies;
`server/Cargo.lock` is the authoritative resolved record for those packages.
Update this registry in the same change that adds, removes, upgrades, or
materially reconfigures a direct production dependency.

Do not pre-approve cross-cutting dependencies. Select and document a dependency
only when a named Milestone behavior requires it and an owning crate is known.

Before approval, each dependency record must identify the Milestone behavior it
enables, its owning crate, its package source and declared version, the minimal
enabled feature set, and why the standard library or an approved dependency is
insufficient. It must also record maintenance and license compatibility
evidence, plus the focused and locked-workspace validation performed.
Security-sensitive dependencies additionally record the security property they
provide, the enabled capabilities or backend choices relevant to that property,
applicable advisory-review evidence, and safe-failure test coverage.

Released crates.io packages are the normal production source. Local paths are
reserved for internal workspace members; third-party code is not vendored into
the workspace. Alternate registries are prohibited unless explicitly approved.
A third-party Git dependency, unpublished fork, or package from another
non-registry source in `server/Cargo.lock` is a temporary exception, whether a
direct production dependency selects it or it is introduced transitively. It
requires an immutable full commit revision where applicable, its source and
replacement rationale, a named owner, and a removal condition or follow-on
issue. It receives the same approval and validation evidence as a released
package. Internal workspace members are not exceptions.

#### `rusqlite`

- **Source and version:** crates.io `=0.40.1`.
- **Owner and behavior:** `weavelit-server-database-sqlite` uses the dependency
  for the Milestone 1 SQLite Application Database connection, configuration,
  health, migration, and transaction behavior. The Rust standard library and
  existing workspace code do not provide a SQLite driver.
- **Features:** default features are disabled and only `bundled` is enabled.
  Runtime extension loading, SQLCipher, URI, UUID, time, statement-cache, WASM,
  and runtime-bindgen features are not enabled. Bundling supplies a consistent
  SQLite implementation without a host shared-library dependency.
- **Maintenance and license:** `rusqlite` 0.40.1 was released on June 6, 2026,
  and its upstream repository remained active at the August 1, 2026 review.
  `rusqlite` and `libsqlite3-sys` use the MIT license; bundled SQLite is in the
  public domain.
- **Advisory review:** the August 1, 2026 GitHub Advisory Database review found
  no advisory matching `rusqlite` 0.40.1 or `libsqlite3-sys` 0.38.1.
- **Safe failure:** the backend excludes URI interpretation, rejects symbolic
  links in the database path, verifies every required connection setting and a
  fixed health query, and maps driver failures to payload-free storage-neutral
  errors without exposing paths, SQL, raw dependency messages, or connection
  settings.
- **Validation:** ten focused real-SQLite package tests cover configuration,
  health, reopen, unavailable storage, invalid database content, symbolic-link
  rejection, literal query-like filenames, invalid paths, and redaction.
  `make -C server check` passes formatting, Clippy with warnings denied, all 17
  locked workspace tests, and locked release builds. The locked feature graph
  and transitive resolution were reviewed for excluded capabilities.

#### `sha2`

- **Source and version:** crates.io `=0.11.0`.
- **Owner and behavior:** `weavelit-server-database-sqlite` uses SHA-256 to bind
  each immutable embedded migration file to its migration-ledger entry.
  `weavelit-module-client-webui` uses it as a build-dependency only, to re-hash
  the Web UI bundle inputs and generated assets at compile time and fail closed
  on a stale embedded bundle; it is not linked into that crate's runtime code.
  `weavelit-server-authentication` uses it for the domain-separated SHA-256
  digest of a session token and of a per-session CSRF token, which is the only
  representation of either value the Application Database stores.
  The standard library and existing approved dependencies do not provide
  SHA-256.
- **Features:** default features are disabled and no optional features are
  enabled. Allocation, object-identifier, and zeroization features are absent;
  the locked graph contains only the digest primitives and CPU-feature support
  required by SHA-256.
- **Maintenance and license:** `sha2` 0.11.0 supports Rust 1.85 and later, and
  its RustCrypto upstream remained active at the August 1, 2026 review. The
  crate uses the MIT or Apache-2.0 license.
- **Advisory review:** the August 1, 2026 GitHub Advisory Database review found
  no advisory matching `sha2` 0.11.0.
- **Safe failure:** the backend hashes exact embedded migration bytes, stores
  the 32-byte digest without logging it, validates every applied ledger entry
  before pending work, and returns only `IntegrityFailure` when migration
  identity, sequence, or checksum cannot be trusted.
- **Validation:** checksum known-vector and registry tests plus seven real-file
  migration tests cover ordered bootstrap, idempotent reopen, unknown, missing,
  reordered, and mismatched history, missing-ledger refusal, schema constraints,
  and transaction rollback. `make -C server check` passes formatting, Clippy
  with warnings denied, all 27 locked workspace tests, and locked release builds.
  The lockfile and feature graph were reviewed for excluded optional features.

#### `base64`

- **Source and version:** crates.io `=0.23.0`.
- **Owner and behavior:** `weavelit-server-lifecycle` uses canonical unpadded
  URL-safe Base64 for keys, nonces, deployment identifiers, locator generations,
  ciphertext, and byte-valued settings in the version 1 JSON formats and
  code-owned locator filenames. `weavelit-server-authentication` uses the same
  canonical unpadded URL-safe engine to encode the 32 random bytes of a session
  token and of a per-session CSRF token. The standard library and approved
  dependencies do not provide Base64 encoding or canonical decoding.
- **Features:** default features are disabled and only `alloc` is enabled. The
  `std` and `simd-unsafe` features are excluded; lifecycle format handling does
  not require architecture-specific unsafe SIMD acceleration.
- **Maintenance, license, and advisories:** version 0.23.0 supports Rust 1.71
  and later and uses the MIT or Apache-2.0 license. The unarchived upstream was
  active at the August 1, 2026 review, and the GitHub Advisory Database review
  found no advisory matching version 0.23.0.
- **Safe failure and validation:** decoding uses only the URL-safe
  no-padding engine, enforces exact decoded lengths and bounds, rejects invalid
  trailing bits and non-canonical text by re-encoding, and never include rejected
  text in errors. Known-answer, invalid alphabet, padding, trailing-bit,
  wrong-length, filename grammar, and redaction tests pass. The locked graph
  excludes `std` and `simd-unsafe`; `make -C server check` passes all 78 tests
  and the locked release build.

#### `chacha20poly1305`

- **Source and version:** crates.io `=0.11.0`.
- **Owner and behavior:** `weavelit-server-lifecycle` uses the RustCrypto
  `XChaCha20Poly1305` implementation to encrypt and authenticate complete
  deployment-record and database-locator payloads for Milestone 1. The Rust
  standard library and approved workspace dependencies do not provide an AEAD
  implementation.
- **Features:** default features are disabled; only `alloc` and `zeroize` are
  enabled. The crate's `getrandom`, reduced-round, `arrayvec`, `bytes`, and
  `rand_core` features are excluded because the lifecycle crate obtains
  fallible operating-system randomness through its direct `getrandom`
  dependency. The selected construction uses a 256-bit key, random 192-bit
  nonce, complete 128-bit tag, and format-defined associated data.
- **Maintenance, license, and advisories:** version 0.11.0 supports Rust 1.85
  and later and uses the MIT or Apache-2.0 license. The unarchived RustCrypto
  AEADs upstream was active at the August 1, 2026 review. The implementation has
  an independent NCC Group audit with no significant findings, and the GitHub
  Advisory Database review found no advisory matching version 0.11.0 or its
  `aead` 0.6.1 abstraction.
- **Safe failure and validation:** authentication completes before
  payload parsing, authentication errors expose no plaintext and collapse to
  one redacted integrity result, tags are never truncated, and nonce generation
  has no weak fallback. The exact published known-answer vector and wrong-key,
  wrong-nonce, wrong-associated-data, tampering, truncation, restart, and
  sensitive-output tests pass. The locked graph contains only `alloc` and
  `zeroize` capabilities; `make -C server check` passes all 78 tests and the
  locked release build.

#### `x25519-dalek`

- **Source and version:** crates.io `=2.0.1`.
- **Owner and behavior:** `weavelit-server-restore` uses X25519 Diffie-Hellman
  to agree the key that unwraps a backup's file key from its age v1 recipient
  stanza, and to derive the recipient public key that binds a submitted
  recovery identity to the artifact it may open. The Rust standard library and
  the approved dependencies provide no elliptic-curve key agreement;
  `chacha20poly1305` is symmetric only.
- **Features:** default features are disabled; only `static_secrets` and
  `zeroize` are enabled. `static_secrets` exposes the reusable `StaticSecret`
  the recovery identity requires, because an ephemeral secret cannot be
  reconstructed from a submitted recovery key. `zeroize` makes `StaticSecret`
  zeroize on drop. `serde`, `getrandom`, `reusable_secrets`, `precomputed-tables`,
  and `alloc` are excluded; Restore never serializes key material, never
  generates keys, and does not need the larger precomputed basepoint tables.
- **Maintenance and license:** the crate declares Rust 1.60 and later and uses
  the BSD-3-Clause license. Its only non-development dependencies in the locked
  graph are `curve25519-dalek` 4.1.3 and `zeroize` 1.9.0, both already resolved
  in `server/Cargo.lock`.
- **Advisory review:** the August 10, 2026 GitHub Advisory Database review found
  no advisory matching `x25519-dalek` 2.0.1. `curve25519-dalek` has
  GHSA-x4gp-pqpj-f43q, a low-severity timing variability in `Scalar29::sub` and
  `Scalar52::sub` affecting versions before 4.1.3; the resolved 4.1.3 is exactly
  that advisory's first patched version, so the locked graph is unaffected.
  GHSA-4hff-hh47-7788 is a duplicate of it. The review also found no advisory
  matching the resolved `curve25519-dalek-derive` 0.1.1, `fiat-crypto` 0.2.9,
  `cpufeatures` 0.2.17, or `subtle`, and confirmed that the two `zeroize_derive`
  advisories (GHSA-c5hx-w945-j4pq, GHSA-r45x-ghr2-qjxc, both fixed in 1.1.1) and
  the two `rand_core` advisories (GHSA-w7j2-35mf-95p7 fixed in 0.6.2,
  GHSA-mmc9-pwm7-qj5w fixed in 0.4.2) predate the resolved `zeroize_derive`
  1.5.0 and `rand_core` 0.6.4.
- **Safe failure:** the agreed secret is rejected when it is non-contributory,
  which refuses a low-order or all-zero ephemeral share before it can produce a
  predictable wrap key. The agreed secret, the wrap key, and the unwrapped file
  key are held in zeroizing buffers, never logged, and never rendered; the
  identity's `Debug` output is redacted. Every agreement or unwrap failure
  collapses to the single `backup_invalid` result.
- **Validation:** the crate's recovery-key, parameter-policy, STREAM, and
  end-to-end validation tests cover identity parsing, recipient derivation,
  a wrong recovery key, a low-order ephemeral share, and redaction. See the age
  v1 Recipient Profile Implementation record for the cross-implementation
  evidence and the locked-workspace result.

#### `hkdf`

- **Source and version:** crates.io `=0.13.0`.
- **Owner and behavior:** `weavelit-server-restore` uses HKDF-SHA-256 to derive
  the three keys the age v1 profile defines: the stanza wrap key from the X25519
  shared secret, the header authenticator key from the file key, and the payload
  key from the file key and payload nonce. The approved `sha2` dependency
  provides only the hash; the extract-and-expand construction with its salt and
  info binding is exactly the kind of primitive that must not be hand-rolled in
  production code.
- **Features:** default features are disabled and no optional feature is
  enabled. `std` is excluded. The locked graph adds only the approved `hmac` and
  `digest` traits the construction is defined over.
- **Maintenance and license:** the crate declares Rust 1.85 and later, uses the
  MIT or Apache-2.0 license, and is published by the RustCrypto KDFs project.
  Version 0.13.0 is required rather than 0.12: 0.12 is defined over `digest`
  0.10, which cannot consume the approved `sha2` 0.11.
- **Advisory review:** the August 10, 2026 GitHub Advisory Database review found
  no advisory matching `hkdf` 0.13.0.
- **Safe failure:** every derivation uses a fixed output length and a
  format-defined label, so no attacker-supplied value selects a derivation
  parameter. Derived keys are held in zeroizing buffers and never leave the
  crate. A derivation cannot fail for the fixed 32-byte output this crate
  requests, and no derived value is ever compared or reported.
- **Validation:** covered by the same Restore test suite and the
  cross-implementation evidence recorded below; the test-only fixture generator
  derives the same three keys independently from `sha2` alone, so a divergence
  in either implementation fails the committed fixture tests.

#### `hmac`

- **Source and version:** crates.io `=0.13.0`.
- **Owner and behavior:** `weavelit-server-restore` uses HMAC-SHA-256 to verify
  the age v1 header authenticator, which is what binds the recipient stanza and
  every declared parameter to the file key before any payload byte is read. It
  is also the primitive `hkdf` is defined over. The approved `sha2` dependency
  provides only the bare hash.
- **Features:** default features are disabled and only `zeroize` is enabled, so
  the authenticator's internal key state is cleared on drop; `std` and `reset`
  are excluded.
- **Maintenance and license:** the crate declares Rust 1.85 and later, uses the
  MIT or Apache-2.0 license, and is published by the RustCrypto MACs project.
  Version 0.13.0 is required for the same `digest` 0.11 compatibility reason as
  `hkdf`.
- **Advisory review:** the August 10, 2026 GitHub Advisory Database review found
  no advisory matching `hmac` 0.13.0 or its `digest` 0.11.3 abstraction.
- **Safe failure:** verification uses the crate's constant-time `verify_slice`
  rather than a byte comparison, the authenticator key is a zeroizing buffer,
  and a mismatch collapses to the single `backup_invalid` result with nothing
  distinguishing it from a wrong recovery key or altered ciphertext. The header
  is authenticated before the payload nonce or any chunk is interpreted.
- **Validation:** the parameter-policy tests assert that an altered header is
  indistinguishable from every other failure, and the cross-implementation
  evidence below exercises the published bad-authenticator vectors.

#### `bech32`

- **Source and version:** crates.io `=0.11.1`.
- **Owner and behavior:** `weavelit-server-restore` uses Bech32 to decode the
  canonical age recovery-key syntax: the lowercase `age1…` recipient with the
  `age` human-readable part and the uppercase `AGE-SECRET-KEY-1…` identity with
  the `AGE-SECRET-KEY-` human-readable part. The standard library and approved
  dependencies provide no Bech32 codec, and the checksum is a correctness
  control, not an encoding convenience.
- **Features:** default features are disabled and only `alloc` is enabled, which
  the checked-encoding entry points require. `std` is excluded.
- **Maintenance and license:** the crate uses the MIT license and is published
  by the rust-bitcoin project. It has no non-development dependencies in the
  locked graph.
- **Advisory review:** the August 10, 2026 GitHub Advisory Database review found
  no advisory matching `bech32` 0.11.1.
- **Safe failure:** decoding uses the checksum-specific `Bech32` entry point
  rather than the permissive decoder, so a Bech32m checksum is rejected. The
  crate additionally requires the exact expected human-readable part, an exact
  32-byte decoded payload, a single case throughout, and byte-for-byte equality
  with a re-encoding of the decoded value, so a mixed-case or otherwise
  non-canonical key is refused as a recovery-key failure before any decryption
  is attempted. Rejected text is never included in an error.
- **Validation:** the crate's recovery-key tests cover canonical identity and
  recipient lines, non-canonical text, mixed case, surrounding whitespace, a
  multi-line submission, an oversize submission rejected before decoding, and
  redacted rendering.

#### age v1 Recipient Profile Implementation

`weavelit-server-restore` implements the [Security Model](../security-model.md)'s
approved age v1 X25519 recipient profile directly on the approved primitives
above rather than depending on the `age` crate.

- **Rationale:** the `age` crate cannot be configured down to the profile
  Weavelit supports. Even with default features disabled it resolves an
  unconditional localization stack (`fluent`, `i18n-embed`, `rust-embed`,
  `walkdir`, `mime_guess`) and unused cryptography (`hpke`, `ml-kem`, `p256`,
  `aes-gcm`, `scrypt`, `pbkdf2`, `sha3`) that Weavelit neither calls nor
  audits. Adopting `age` added 116 package entries to `server/Cargo.lock`,
  resolving the workspace from 143 to 259 entries. Implementing the profile on
  the approved primitives instead adds 13 entries, for 156 in total, so it
  avoids 103 packages. This directly serves the requirement that every
  production dependency be minimal, justified, and reviewable.
- **Scope:** the implementation is a reader only; the Server never encrypts a
  backup. It accepts exactly one X25519 recipient stanza. An `scrypt` stanza,
  any other or unknown stanza type, an absent recipient stanza, an additional
  stanza, and an unsupported version line are refused as `backup_incompatible`
  before key agreement, because Weavelit's backup format defines a single
  recovery recipient. This is deliberately narrower than the age specification,
  which permits additional and unknown stanzas.
- **Composition:** no cryptographic primitive is hand-written. Key agreement,
  key derivation, authentication, and encryption come from `x25519-dalek`,
  `hkdf`, `hmac`, `sha2`, and `chacha20poly1305`; the crate contributes only the
  format framing, bounds, and policy. `#![forbid(unsafe_code)]` remains in
  force.
- **Safe failure:** the header is bounded before it is scanned and the payload
  is authenticated chunk by chunk, so no allocation and no plaintext are
  produced from unauthenticated attacker-controlled length. A wrong recovery
  key, an altered header, altered ciphertext, an altered tag, a truncated
  stream, a dropped or unflagged final chunk, and trailing garbage all collapse
  to one indistinguishable `backup_invalid` result.
- **Cross-implementation validation:** the committed fixtures in `tests/fixtures/`
  are produced by a deliberately independent second implementation of the same
  profile in `tests/support/`, which derives HKDF and HMAC by hand from `sha2`
  and shares no code with the reader, so the committed fixture bytes bind the
  two together. Because both live in this repository, they are not external
  validation on their own. External validation comes from the C2SP Community
  Cryptography Test Vectors for age, vendored under `tests/vectors/` from
  <https://github.com/C2SP/CCTV> path `age/testdata` at commit
  `1e3d2860d46e94e777e1b17c7a6f2436387e3ecc`, retrieved August 10, 2026 under
  the Zero-Clause BSD option of the upstream license. Upstream holds 143
  vectors; the 33 `armored: yes` vectors are excluded because the Weavelit
  backup format defines no ASCII armor, and the remaining 110 are vendored byte
  for byte with a `vectors.json` manifest pinning each file's length and
  SHA-256. `src/vectors.rs`, compiled only under `cfg(test)`, wraps each age
  body in the fixed outer envelope and runs the production reader over all 110,
  pinning the outcome of every one:
  - Upstream partition: 19 `success`, 60 `header failure`, 18 `payload failure`,
    12 `no match`, and 1 `HMAC failure`.
  - Reader partition: 9 decrypt and match their expected payload SHA-256, 58 are
    refused as `backup_invalid`, and 43 are refused as `backup_incompatible`.
  - All 43 `backup_incompatible` results are the deliberate policy exclusions
    above: 25 `scrypt` passphrase vectors, 12 `stanza_*` vectors carrying an
    additional or unknown recipient stanza, 3 grease or multi-recipient vectors
    (`hybrid_grease`, `x25519_grease`, `x25519_multiple_recipients`), the
    non-canonically cased stanza type in `x25519_lowercase`, and the two
    rejected version lines in `version_unsupported` and `header_crlf`.
  - Ten of the 19 upstream `success` vectors are refused because they are
    outside the approved profile: 7 as `backup_incompatible` (`scrypt`,
    additional stanzas, or multiple recipients) and the 3 hybrid post-quantum
    vectors as `backup_invalid`, because their single recipient stanza line is
    longer than the reader's 1024-byte bounded header scan and so is refused as
    malformed before the stanza type is examined.
  - The multi-chunk STREAM vectors up to 258 chunks all execute, including the
    unflagged final chunk, duplicate final chunk, short second chunk, empty last
    chunk, and trailing-garbage cases that the single-chunk fixture generator
    cannot reach. No vector revealed a correctness or safety failure in the
    reader, and no production bound was relaxed to accommodate one.
- **Validation:** `make -C server check` passes formatting, Clippy with warnings
  denied, all locked workspace tests, and locked release builds. The locked
  feature graph was reviewed on August 10, 2026 for the excluded capabilities
  named in each record above; that review used `cargo tree`, `server/Cargo.lock`,
  and the vendored crate manifests only.

#### `rustls`

- **Source and version:** crates.io `=0.23.43`.
- **Owner and behavior:** `weavelit-server` uses Rustls to construct the direct
  TLS configuration from trusted host PEM material for the Milestone 1 HTTPS
  listener. The Rust standard library and approved workspace dependencies do
  not parse PEM material, validate certificate and private-key compatibility,
  or provide a TLS server configuration.
- **Features:** default features are disabled; only `aws_lc_rs`, `std`, and
  `tls12` are enabled. `aws_lc_rs` selects the maintained AWS-LC cryptographic
  provider; `std` supplies the host process integration required by the runtime;
  and `tls12` permits the required TLS 1.2 and TLS 1.3 configuration. Logging,
  post-quantum preference, compression, `ring`, FIPS, custom-provider, and
  additional I/O capabilities are excluded. The runtime uses Rustls'
  maintained `rustls-pki-types` API for bounded PEM sections and does not depend
  on the archived `rustls-pemfile` crate.
- **Maintenance, license, and advisories:** version 0.23.43 was released July
  29, 2026, supports Rust 1.71 and later, and uses Apache-2.0, ISC, or MIT
  licensing. The Rustls upstream published that release during the August 2,
  2026 review. OSV queries on August 2, 2026 returned no advisory for Rustls
  0.23.43 or its resolved AWS-LC provider `aws-lc-rs` 1.17.3. The review
  rejected `rustls-pemfile` because OSV reports RUSTSEC-2025-0134: its upstream
  is archived and unmaintained.
- **Safe failure and validation:** the runtime accepts only one numeric
  nonzero listener address, bounds each PEM file before parsing, rejects unsafe
  filesystem entries and unsupported PEM sections, verifies the certificate and
  private key through the selected provider, and maps every material failure to
  a fixed payload-free configuration result. It neither binds a socket nor
  exposes a listener in this validation boundary. Focused tests cover valid,
  invalid-address, missing, unreadable, symbolic-link, malformed, mismatched,
  and process-level pre-lifecycle failures. `cargo test --locked -p
  weavelit-server --test startup` passes all 23 tests; the locked feature graph
  contains only the selected Rustls provider capabilities.

#### HTTPS Runtime Composition

The following crates.io packages are direct dependencies of `weavelit-server`
for the Milestone 1 single direct-TLS listener. The Rust standard library and
the approved Rustls dependency do not provide HTTP routing, bounded HTTP/1
header parsing, response-body collection, or asynchronous socket and TLS-stream
handling.

| Package | Exact version and minimal features | Owner and purpose | Maintenance, license, and advisory evidence |
| --- | --- | --- | --- |
| `axum` | `=0.8.9`; defaults disabled; `http1`, `tokio` | `weavelit-server` composes the restricted status, Application Database selection, and embedded-asset routes; `weavelit-module-client` translates the status and selection requests and JSON responses; `weavelit-module-client-webui` translates each embedded asset into its profile-bounded response | Tokio-rs Axum; MIT. The cached package metadata identifies its upstream repository. No advisory scanner is installed in the development container, so no clean-advisory assertion is recorded. |
| `http-body-util` | `=0.1.4`; defaults enabled | `weavelit-server`; collects each Axum route response before direct TLS emission, bounded by its `ResponseProfile` body limit (128 B JSON, 16 KiB HTML, 256 KiB JavaScript, 64 KiB CSS) rather than a single fixed size | Hyperium; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `httparse` | `=1.10.1`; defaults enabled | `weavelit-server`; bounded HTTP/1 request-head parsing before route dispatch, with request-body buffering bounded separately to 1 KiB and permitted only for `PUT` | Sean McArthur; MIT OR Apache-2.0. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tokio` | `=1.53.1`; defaults disabled; `io-util`, `macros`, `net`, `rt-multi-thread`, `sync`, `time` | `weavelit-server`; bounded asynchronous listener, TLS-stream I/O, timers, and task runtime | Tokio; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tokio-rustls` | `=0.26.4`; defaults disabled | `weavelit-server`; asynchronous stream adapter for the already-approved Rustls configuration | Rustls; MIT OR Apache-2.0. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tower` | `=0.5.3`; defaults disabled; `util` | `weavelit-server`; invokes the Axum route service for the status, selection, and embedded-asset routes after bounded request-head validation | Tower; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |

These packages do not enable HTTP/2, compression, CORS, cookie, form, JSON,
query, tracing, client, proxy, or alternate TLS-provider features. The locked
resolution records only crates.io sources and exact checksums. Contract tests
cover both status projections, lifecycle route removal, fixed rejection bodies,
and bind-failure redaction; the full locked workspace gate remains required for
every dependency-resolution change.

#### `getrandom`

- **Source and version:** crates.io `=0.4.3`.
- **Owner and behavior:** `weavelit-server-lifecycle` obtains operating-system
  randomness for the deployment key, deployment identifier, locator generation,
  temporary-file uniqueness, and AEAD nonces. `weavelit-server` obtains the same
  randomness for the Restore-result System Log record identifier and its
  correlation identifier, which must be unpredictable and must not derive from
  request content. `weavelit-server-authentication` obtains the random salt for
  a decoy or replacement password verifier and the 32 bytes of entropy behind
  each session and CSRF token. The Rust standard library and approved
  dependencies do not
  expose the required fallible operating-system random-byte interface.
- **Features:** default features are disabled and no optional features are
  enabled. The `std`, `sys_rng`, and `wasm_js` features are excluded; Milestone
  1 uses the supported Ubuntu operating-system source and does not add a user-
  supplied random-number generator or browser target.
- **Maintenance, license, and advisories:** version 0.4.3 supports Rust 1.85 and
  later and uses the MIT or Apache-2.0 license. The unarchived `getrandom`
  upstream was active at the August 1, 2026 review, and the GitHub Advisory
  Database review found no advisory matching version 0.4.3.
- **Safe failure and validation:** any operating-system randomness
  failure stops key, identifier, nonce, or temporary-file creation without a
  deterministic or lower-quality fallback. Focused failure injection proves the
  payload-free unavailable category and no fallback; first-start, restart,
  locator replacement, and temporary-file tests exercise nonzero random values.
  The locked graph excludes optional features; `make -C server check` passes all
  78 tests and the locked release build.

#### `rustix`

- **Source and version:** crates.io `=1.1.4`.
- **Owner and behavior:** `weavelit-server-lifecycle` uses safe Unix APIs to
  inspect the effective identity, set the owner-only umask, traverse the
  absolute state-root path component by component without following symbolic
  links, inspect ownership, mode, type, and hard-link count, and perform
  directory-relative creation, replacement, removal, and synchronization.
  `weavelit-server` uses the same descriptor-relative no-follow primitives to
  open and validate configured TLS material. The standard library does not
  expose the complete race-resistant relative Unix filesystem API without
  platform constants or unsafe calls. The Rust standard library separately
  supplies the process-lifetime file lock.
- **Features:** default features are disabled; only `std`, `fs`, and `process`
  are enabled. Networking, mount, asynchronous I/O, memory-management, terminal,
  thread, timing, latest-Linux opt-in, and explicit libc-backend features are
  excluded.
- **Maintenance, license, and advisories:** version 1.1.4 supports Rust 1.63 and
  later and uses Apache-2.0 with LLVM exception, Apache-2.0, or MIT licensing.
  The unarchived Bytecode Alliance upstream was active at the August 1, 2026
  review, and the GitHub Advisory Database review found no advisory matching
  version 1.1.4.
- **Safe failure and validation:** no operation follows a state-root or
  child symlink or falls back from a failed ownership, mode, type, link-count,
  atomic-replacement, or synchronization check. Isolated real-filesystem tests
  cover final and intermediate symlinks, exact root and file modes, regular-file
  and hard-link checks, closed inventory and cardinality, process locking,
  write/sync/rename/directory-sync failures, cleanup, and redacted mapping. The
  locked graph enables only `std`, `fs`, and `process`; `make -C server check`
  passes all 78 tests and the locked release build.

#### `zeroize`

- **Source and version:** crates.io `=1.9.0`.
- **Owner and behavior:** `weavelit-server-lifecycle` uses `Zeroizing` and the
  `Zeroize` trait for application-owned at-rest key and decrypted anchor buffers.
  `weavelit-server` uses `Zeroizing` for the owned backup artifact and recovery
  key a Restore takes custody of, so both are cleared when the orchestration
  releases them rather than surviving in a runtime buffer.
  `weavelit-server-authentication` uses `Zeroizing` for the encoded session and
  CSRF token text, which exists only long enough to reach the response that
  carries it. The standard library
  does not guarantee that clearing sensitive memory will survive compiler
  optimization.
- **Features:** default features are disabled and only `alloc` is enabled. The
  derive, Serde, SIMD, architecture-specific, and `std` features are excluded.
- **Maintenance, license, and advisories:** version 1.9.0 supports Rust 1.85 and
  later and uses the MIT or Apache-2.0 license. The unarchived RustCrypto
  utilities upstream was active at the August 1, 2026 review, and the GitHub
  Advisory Database review found no advisory matching version 1.9.0.
- **Safe failure and validation:** sensitive owned buffers are
  zeroized on normal and error exits without claiming protection against
  unavoidable copies, process memory inspection, swapping, or host compromise.
  The key wrapper and every decrypted plaintext allocation use `Zeroizing`;
  successful, wrong-key, tampered, malformed, and restart paths exercise their
  drop behavior. The locked graph enables only `alloc`; `make -C server check`
  passes all 78 tests and the locked release build.

#### `serde`

- **Source and version:** crates.io `=1.0.229`.
- **Owner and behavior:** `weavelit-server-lifecycle` derives the bounded
  versioned key-file, envelope, deployment-record, and database-locator data
  models used by the strict JSON parser. `weavelit-module-client` derives the
  bounded Application Database selection request model. The standard library
  does not provide structured serialization or deserialization.
- **Features:** default features are disabled; only `derive` and `std` are
  enabled. Reference-counted-value and unstable features are excluded.
- **Maintenance, license, and advisories:** version 1.0.229 supports Rust 1.56
  and later and uses the MIT or Apache-2.0 license. The unarchived Serde
  upstream was active at the August 1, 2026 review, and the GitHub Advisory
  Database review found no advisory matching version 1.0.229.
- **Safe failure and validation:** every anchor model denies unknown
  fields and validates lengths, versions, enum values, and binary encodings before
  domain construction. Duplicate, unknown, missing, reordered, invalid enum,
  wrong-length, oversized, and malformed model tests pass. The locked graph
  enables only `derive` and `std`; `make -C server check` passes all 78 tests and
  the locked release build.

#### `serde_json`

- **Source and version:** crates.io `=1.0.151`.
- **Owner and behavior:** `weavelit-server-lifecycle` parses and emits the
  bounded, versioned UTF-8 JSON anchor formats through typed Serde models.
  `weavelit-module-client` parses the bounded Application Database selection
  request body through a strict typed model.
  `weavelit-module-client-webui` uses it as a build-dependency only, to parse
  the Web UI build content manifest strictly at compile time; it is not linked
  into that crate's runtime code. The standard library does not provide a JSON
  parser, and ad hoc string parsing is prohibited for the security-sensitive
  formats.
- **Features:** default features are disabled and only `std` is enabled.
  Arbitrary-precision numbers, float round-trip, order preservation, raw values,
  and unbounded-depth parsing are excluded.
- **Maintenance, license, and advisories:** version 1.0.151 supports Rust 1.71
  and later and uses the MIT or Apache-2.0 license. The unarchived Serde JSON
  upstream was active at the August 1, 2026 review, and the GitHub Advisory
  Database review found no advisory matching version 1.0.151.
- **Safe failure and validation:** file-size bounds apply before parse;
  authenticated plaintext is parsed with bounded typed structures; trailing
  content, duplicate or unknown fields, unsupported versions, and malformed
  input fail closed without raw parser output. The exact deterministic writer
  vector and whitespace, ordering, trailing-content, invalid UTF-8, size,
  malformed-input, and redaction tests pass. The locked graph enables only
  `std`; `make -C server check` passes all 78 tests and the locked release build.

#### `argon2`

- **Source and version:** crates.io `=0.5.3`.
- **Owner and behavior:** `weavelit-server-authentication` uses the RustCrypto
  Argon2 implementation for the
  [Authentication Design](authentication/authentication-design.md)'s Argon2id
  password-hashing profile: parsing a stored PHC verifier, verifying a
  submitted password against an allowlisted profile, and producing a
  replacement verifier at the current profile. Argon2 is a memory-hard
  password-hashing function whose correctness and side-channel behavior must
  not be hand-written; the Rust standard library and every other approved
  dependency provide no password-hashing function.
- **Features:** default features are disabled; only `alloc`, `password-hash`,
  and `zeroize` are enabled. `alloc` and `password-hash` supply the PHC string
  parser and writer the stored format requires, and `zeroize` clears the
  algorithm's internal block memory on drop. `std`, `rand`, `simple`, and
  `parallel` are excluded: the crate obtains fallible operating-system
  randomness through its direct `getrandom` dependency, never uses the
  convenience wrappers, and runs the approved `p=1` profile with no thread
  pool.
- **Maintenance and license:** `argon2` 0.5.3 was published on January 20, 2024,
  declares Rust 1.65 and later, and uses the MIT or Apache-2.0 license. It is
  published by the RustCrypto password-hashes project. The stable 0.5 line is
  selected deliberately over the 0.6 release-candidate line; a pre-release is
  not an approved production source. The consequence is that the locked graph
  carries the `digest` 0.10 stack (`blake2` 0.10.6, `digest` 0.10.7,
  `block-buffer` 0.10.4, `crypto-common` 0.1.7, `generic-array` 0.14.7)
  alongside the `digest` 0.11 stack the approved `sha2` 0.11 already resolves.
  The duplication is accepted for this release; adopting the 0.6 line once it
  is released is a separate, reviewed dependency change.
- **Advisory review:** the August 11, 2026 OSV review found no advisory matching
  `argon2` 0.5.3, `password-hash` 0.5.0, `base64ct` 1.8.3, `digest` 0.10.7,
  `block-buffer` 0.10.4, `crypto-common` 0.1.7, or `version_check` 0.9.5.
  `blake2` has RUSTSEC-2019-0019, an incorrect-HMAC-block-size defect affecting
  versions before 0.8.1; the resolved 0.10.6 is unaffected. `generic-array` has
  RUSTSEC-2020-0146, an unsoundness in the `arr!` macro affecting versions
  before 0.13.3; the resolved 0.14.7 is unaffected.
- **Safe failure:** a stored verifier is parsed and matched against the closed
  profile allowlist before Argon2 is invoked, and the engine re-checks that
  match immediately before running, so the cost parameters encoded in a stored
  value can never select the memory one verification allocates. Every
  allowlisted profile stays within the approved 64 MiB verification ceiling. A
  malformed verifier, an unlisted profile, an unknown account, an inactive
  account, and an account with no verifier are all denied after one
  verification against a decoy verifier built at the current profile, so no
  denial is distinguishable from a wrong password. A hashing failure is a
  payload-free error that never carries a password, a salt, or rejected PHC
  text.
- **Validation:** thirty focused package tests cover the approved profile
  constants, fourteen rejected non-allowlisted encodings including the
  approximately 4 GiB `m=4194304,t=100,p=16` verifier a hostile backup could
  carry, policy construction above the verification ceiling, engine refusal of
  a verifier outside its own policy, real Argon2 verification and rehashing at
  a test profile, and an injected operation-counting engine that proves every
  denial path performs one verification of identical shape and produces no
  replacement verifier. `make -C server check` passes formatting, Clippy with
  warnings denied, all locked workspace tests, and the locked release build.

#### `subtle`

- **Source and version:** crates.io `=2.6.1`, pinned in `[workspace.dependencies]`.
- **Owner and behavior:** `weavelit-server-authentication` compares a stored
  session or CSRF token digest against a submitted one without a
  data-dependent branch or early return. `weavelit-server-database` uses the
  same trait for the stored session and CSRF digest types in its live session
  contract, so the decision to accept a stored row as the presented session is
  constant time. Two workspace crates now require the package, so its shared
  configuration is owned by the workspace manifest. The standard library's
  `PartialEq` for byte arrays is permitted to short-circuit, which would leak
  digest prefix agreement through timing. The package is already in the locked
  graph as a transitive dependency of `password-hash` and `curve25519-dalek`;
  this record approves it as a direct dependency.
- **Features:** default features are disabled and no optional feature is
  enabled. `std`, `i128`, `const-generics`, and the nightly
  `core_hint_black_box` feature are excluded; the crate uses only the
  `ConstantTimeEq` trait over fixed-size byte arrays.
- **Maintenance and license:** version 2.6.1 uses the BSD-3-Clause license and
  is published by the dalek-cryptography project. It has no non-development
  dependencies in the locked graph.
- **Advisory review:** the August 11, 2026 OSV review found no advisory matching
  `subtle` 2.6.1.
- **Safe failure:** comparison returns a `Choice` that is converted to a `bool`
  only at the decision point, and a digest type implements neither `PartialEq`
  nor `Display`, so constant-time comparison is the only comparison a caller can
  reach and no digest can be rendered into a log or a response.
- **Validation:** the session-token tests cover a matching digest, an empty
  token, a different token, a single altered character, a digest reconstructed
  from stored bytes, and domain separation between the session and CSRF digest
  domains.

#### `totp-rs`

- **Source and version:** crates.io `=6.0.0`.
- **Owner and behavior:** `weavelit-module-mfa-totp` uses the package to derive
  and verify a time-based one-time password under the
  [Authentication Design](authentication/authentication-design.md)'s approved
  RFC 6238 profile: HMAC-SHA-1, six digits, a thirty-second step, a `T0` of
  zero, a 160-bit secret, and acceptance of the current step plus or minus one.
  The module also uses the package's unpadded RFC 4648 Base32 encoding to
  render an enrollment secret. RFC 6238 code derivation and its constant-time
  comparison must not be hand-written, and no other approved dependency
  provides a one-time-password construction.
- **Features:** default features are disabled; only `alloc` and `zeroize` are
  enabled. `alloc` supplies the Base32 encoding the enrollment secret requires,
  and `zeroize` clears the secret material the engine holds on drop. `otpauth`
  is excluded specifically: it would add `url`, whose `idna` dependency carries
  the full ICU normalization stack into the Server for the sole purpose of
  formatting one fixed provisioning string. The module builds that string by
  direct formatting over the already-approved `percent-encoding` package
  instead, which the excluded feature would have pulled in anyway. `qr` is
  excluded because it implies `otpauth` and adds image generation the Server
  does not perform; `gen_secret` because the Server supplies the twenty secret
  bytes from the operating-system random source rather than delegating
  generation; `std` and the default `migration` feature because neither the
  clock nor the legacy conversion path is used; `serde` because a secret is
  never serialized; and `steam` because that non-RFC variant is not offered.
  The locked graph therefore adds only `base32` 0.5.1, `constant_time_eq`
  0.4.2, and `sha1` 0.11.0, reusing the approved `sha2` 0.11, `hmac` 0.13,
  `digest` 0.11, and `zeroize` 1.9 stacks already resolved.
- **Maintenance and license:** version 6.0.0 declares Rust 1.88 and later and
  uses the MIT license. The single consumer is `weavelit-module-mfa-totp`, so
  the pin stays in that crate manifest rather than in
  `[workspace.dependencies]`.
- **Advisory review:** the August 11, 2026 OSV review found GHSA-8vxv-2g8p-2249
  for `totp-rs`, a non-constant-time secret comparison affecting versions before
  1.1.0; the pinned 6.0.0 is unaffected and compares through
  `constant_time_eq`. The review found no advisory matching `base32` 0.5.1,
  `constant_time_eq` 0.4.2, or `sha1` 0.11.0. SHA-1 is used only as the HMAC
  primitive RFC 6238 and every authenticator application require, where its
  collision weakness does not apply.
- **Safe failure:** the secret and the provisioning URI never leave the module
  as plain values. Both are carried in zeroizing wrappers that redact in
  `Debug`, implement no `Display` and no `PartialEq`, and expose their contents
  only through an explicit accessor, so neither can reach a log, an error, or a
  response body by accident; in particular the package's own `Display` for a
  secret, which renders hexadecimal, is never delegated. A rejected issuer or
  account name produces a payload-free error that does not echo the input.
  Verification takes the current time as a parameter and the module reads no
  clock, so a caller cannot be surprised by ambient time.
- **Validation:** the module's tests pin the four RFC 6238 SHA-1 test vectors,
  the exact unpadded Base32 encoding of the RFC secret, the exact provisioning
  URI including an account name whose characters must be percent-encoded, the
  boundary of the acceptance window at plus or minus one step and its rejection
  at two, code stability across a whole step, rejection of malformed and
  wrong-length codes, rejection under an altered secret, and the redaction of
  every secret-bearing type. `make -C server check` passes formatting, Clippy
  with warnings denied, all locked workspace tests, and the locked release
  build.

The workspace manifest owns an approved shared dependency's identity, version,
source, and any workspace-wide security baseline. A single-consumer dependency
remains in its owning crate manifest. When a second workspace crate requires
the same package, that change promotes its shared configuration to
`[workspace.dependencies]`.

### Shared Dependency Versions And Features

Each consuming crate explicitly declares only the minimal features needed for
its behavior. The approval record states whether default features are enabled;
when the upstream package supports it and required behavior permits, use
`default-features = false` and opt into named features instead. Review every
enabled feature as part of the dependency change because Cargo unifies features
across workspace consumers.

### Dependency Resolution And Updates

Commit `server/Cargo.lock` as Cargo-generated output and never edit it by hand.
A dependency manifest change that changes resolution includes the resulting
lockfile update in the same change. Normal updates are targeted with Cargo and
reviewed for all resolved package, version, and source changes. A broad update
is a separately described dependency-maintenance change, not incidental feature
work.

Run the locked workspace validation required by the
[Testing and Validation Policy](../testing.md) for every dependency-resolution
change. A security update may be expedited, but its record identifies the
advisory or upstream notice, resolved version, affected behavior, lockfile
impact, and validation performed.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
- [Server Lifecycle Design](lifecycle/lifecycle-design.md)
- [Lifecycle Anchor Protection And Serialization Profile](lifecycle/lifecycle-anchor-profile-decision.md)
- [Server Init Design](lifecycle/init/init-design.md)
- [Server Restore Design](lifecycle/restore/restore-design.md)
- [Application Database Design](database/application-database-design.md)
- [Log Module Design](../log-modules/log-module-design.md)
- [Testing and Validation Policy](../testing.md)
