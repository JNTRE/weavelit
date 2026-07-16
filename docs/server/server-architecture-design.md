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

Internal Server Rust crates use this package-name convention:

```text
weavelit-server-<component>[-<specific-component>]
```

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
such as `weavelit-server-log-sqlite`, without
requiring a `weavelit-server-log` crate before it has a meaningful shared
contract or code.

## Compiled-In Component Boundaries

The **[Weavelit Server](../glossary.md#applications-and-interfaces)** composes
supported **[Application Database](../glossary.md#applications-and-interfaces)**
backends and runtime modules as compiled-in Rust crates. It owns backend and
module selection, common configuration validation, and lifecycle behavior.
Component crates own their implementation-specific behavior, including
validation of their connection and storage settings, behind their documented
boundaries.

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
evidence, plus the focused and locked-workspace validation performed. Security-
sensitive dependencies additionally record their security property, relevant
capabilities or backend choices, applicable advisory review, and safe-failure
test coverage.

Released crates.io packages are the normal production source. Local paths are
reserved for internal workspace members; third-party code is not vendored into
the workspace. Alternate registries are prohibited unless explicitly approved.
A third-party Git dependency, unpublished fork, or package from another
non-registry source resolved in `server/Cargo.lock` is a temporary exception.
It requires an immutable full commit revision where applicable, its source and
replacement rationale, a named owner, and a removal condition or follow-on
issue. It receives the same approval and validation evidence as a released
package. Internal workspace members are not exceptions.

| Package | Source and version | Owning crate | Behavior | Enabled features and security baseline |
| --- | --- | --- | --- | --- |
| `rusqlite` | crates.io; exact version is recorded when the dependency is first declared | `weavelit-server-database-sqlite` | Milestone 1 SQLite Application Database backend | `bundled`; do not enable runtime SQLite extension loading; select only additional features required by the backend |

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

- [Core Statements](../core-statements.md)
- [Glossary](../glossary.md)
- [Application Database Design](database/application-database-design.md)
- [Log Module Design](../log-modules/log-module-design.md)
- [Testing and Validation Policy](../testing.md)
