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
justified. The naming convention does not classify a component as a runtime
module.

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

The shared Log Module contract is `weavelit-server-log`. It owns the Server
core's typed record and dispatch boundary, not log-record construction or a
destination implementation. `weavelit-module-log-sqlite` and a future
`weavelit-module-log-mysql` may implement that contract while retaining their
own persistence and delivery behavior.

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

The compiled-in `weavelit-module-client-webui` crate owns translation of the
Web UI pre-operational status request and response contract. The runtime mounts
that route only when the Server-owned lifecycle gate permits it and retains
ownership of direct TLS, listener composition, raw request parsing, resource
limits, and lifecycle classification. The module cannot independently compose a
route or listener. The [Web UI Pre-Operational Status Surface](../client-modules/web-ui/pre-operational-status-design.md)
defines its public contract and resource limits.

The implementation selects minimal features and exact crates.io versions for
Axum, Hyper, Tokio, and their required adapters under the dependency policy
below. Each selected package must be maintained and advisory-reviewed before it
is added. A package upgrade is a deliberate dependency change that repeats the
version, change, advisory, and validation review; no future version is approved
by this architecture decision.

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
  each immutable embedded migration file to its migration-ledger entry. The
  standard library and existing approved dependencies do not provide SHA-256.
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
  code-owned locator filenames. The standard library and approved dependencies
  do not provide Base64 encoding or canonical decoding.
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
  weavelit-server --test startup` passes all 12 tests; the locked feature graph
  contains only the selected Rustls provider capabilities.

#### HTTPS Runtime Composition

The following crates.io packages are direct dependencies of `weavelit-server`
for the Milestone 1 single direct-TLS listener. The Rust standard library and
the approved Rustls dependency do not provide HTTP routing, bounded HTTP/1
header parsing, response-body collection, or asynchronous socket and TLS-stream
handling.

| Package | Exact version and minimal features | Owner and purpose | Maintenance, license, and advisory evidence |
| --- | --- | --- | --- |
| `axum` | `=0.8.9`; defaults disabled; `http1`, `tokio` | `weavelit-server` composes the fixed restricted route; `weavelit-module-client-webui` translates the status request and JSON response | Tokio-rs Axum; MIT. The cached package metadata identifies its upstream repository. No advisory scanner is installed in the development container, so no clean-advisory assertion is recorded. |
| `http-body-util` | `=0.1.4`; defaults enabled | `weavelit-server`; collects the fixed, sub-128-byte Axum route response before direct TLS emission | Hyperium; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `httparse` | `=1.10.1`; defaults enabled | `weavelit-server`; bounded HTTP/1 request-head parsing before route dispatch, without request-body buffering | Sean McArthur; MIT OR Apache-2.0. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tokio` | `=1.53.1`; defaults disabled; `io-util`, `macros`, `net`, `rt-multi-thread`, `sync`, `time` | `weavelit-server`; bounded asynchronous listener, TLS-stream I/O, timers, and task runtime | Tokio; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tokio-rustls` | `=0.26.4`; defaults disabled | `weavelit-server`; asynchronous stream adapter for the already-approved Rustls configuration | Rustls; MIT OR Apache-2.0. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |
| `tower` | `=0.5.3`; defaults disabled; `util` | `weavelit-server`; invokes the fixed Axum route service after bounded request-head validation | Tower; MIT. The cached package metadata identifies its upstream repository. Advisory scanning was unavailable. |

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
  temporary-file uniqueness, and AEAD nonces. The Rust standard library and
  approved dependencies do not expose the required fallible operating-system
  random-byte interface.
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
  The standard library does not guarantee that clearing sensitive memory will
  survive compiler optimization.
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
  models used by the strict JSON parser. The standard library does not provide
  structured serialization or deserialization.
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
  bounded, versioned UTF-8 JSON anchor formats through typed Serde models. The
  standard library does not provide a JSON parser, and ad hoc string parsing is
  prohibited for the security-sensitive formats.
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
