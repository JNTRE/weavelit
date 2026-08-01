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
seal reconciliation. The runtime supplies its compiled-in Application Database
backend catalog and uses the lifecycle result to choose which routes may exist.
The lifecycle crate does not create new application state, interpret backup
contents, handle a private recovery key, or implement client presentation.

`weavelit-server-init` owns only the new-state workflow. It uses the lifecycle
crate to select and reopen the Application Database and to validate and advance
trusted lifecycle state. It owns initialization requests, first-user and
Administrators Group creation, initial Log Module configuration and assignment,
recovery-key generation and delivery, proof verification, the atomic creation of
new application state, and durable Init-result delivery through the committed
System Log assignment. Its detailed workflow is defined in the
[Server Init Design](lifecycle/init/init-design.md).

`weavelit-server-restore` owns only the existing-state workflow. It uses the
lifecycle crate to select and reopen an eligible Application Database and to
validate and advance trusted lifecycle state. It owns bounded encrypted backup
staging, backup and recovery-key validation, authenticated decryption, format
and compatibility validation, restored-session invalidation, protected-secret
re-encryption, atomic restoration, and durable Restore-result delivery through
the restored System Log assignment. It never exposes the private
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

### Planned Production Dependency Candidates

No production dependency candidates are currently selected.

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
- [Server Init Design](lifecycle/init/init-design.md)
- [Server Restore Design](lifecycle/restore/restore-design.md)
- [Application Database Design](database/application-database-design.md)
- [Log Module Design](../log-modules/log-module-design.md)
- [Testing and Validation Policy](../testing.md)
