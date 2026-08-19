# Server Component Inventory Crate Agent Guide

This crate defines the neutral compiled-in component inventory that every
pre-operational Weavelit Server workflow judges submitted or requested state
against: the Client, MFA, Service, and Log Modules and the named Operations a
build actually compiles in.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns the `AvailableComponents` representation and its membership
  queries, including the `MfaFactorFormat` an MFA Module declares alongside its
  name and the `LogSettingsFormat` a Log Module declares alongside its own.
- It exists so Init and Restore share one inventory value without depending on
  each other, and so neither owns a type the other must reach through.
- It reuses `weavelit-server-database`'s bounded `Name` and takes no other
  workspace path dependency.
- It does not own the runtime derivation of the inventory. The `weavelit-server`
  runtime derives the inventory once from the compiled-in module crates'
  identifier constants and registrations and supplies it as an inbound value, so
  this crate never depends on a Client, MFA, Service, or Log Module
  implementation.
- It does not own compatibility rules, workflow validation order, error
  presentation, or module registration.

## Asset Inventory

- `Cargo.toml`: Package metadata and its single Application Database contract
  dependency.
- `src/lib.rs`: The `AvailableComponents` value, the `MfaFactorFormat` and
  `LogSettingsFormat` it carries, its membership queries, and their unit tests.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Architecture Design, Server Init Design, and Server Restore
  Design before changing what the inventory represents.
- Agents MUST NOT restate a module identifier as a string literal in production code here
  or in a consumer; the runtime derivation reads each module crate's
  `MODULE_IDENTIFIER` so a compiled-in module and the inventory it is judged by
  cannot drift apart.
- MUST prefer the runtime's production inventory constructor in an end-to-end test.
  A test that supplies its own inventory proves only what that inventory
  contains, not what the shipped binary serves.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- MUST keep the crate free of workflow, transport, persistence, and presentation
  behavior; it carries a value, not a policy.
- MUST keep every dependency exactly pinned with `default-features = false` and the
  minimum feature set the crate requires.
- MUST forbid `unsafe` code.
