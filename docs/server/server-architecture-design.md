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
module selection, configuration validation, and lifecycle behavior; component
crates own their implementation-specific behavior behind their documented
boundaries.

A shared Server crate boundary must not erase the distinction between product
concepts. In particular, an Application Database backend is not a runtime
module. Application Database persistence remains separate from every
**[Log Module](../glossary.md#applications-and-interfaces)** destination even
when their implementations use the same technology.

## Related Documents

- [Core Statements](../core-statements.md)
- [Glossary](../glossary.md)
- [Application Database Design](database/application-database-design.md)
- [Log Module Design](../log-modules/log-module-design.md)
- [Testing and Validation Policy](../testing.md)
