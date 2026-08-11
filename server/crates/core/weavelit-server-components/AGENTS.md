# Server Component Inventory Crate Agent Guide

This crate defines the neutral compiled-in component inventory that every
pre-operational Weavelit Server workflow judges submitted or requested state
against: the Client, MFA, Service, and Log Modules and the named Operations a
build actually compiles in.

## Purpose and Scope

- This crate owns the `AvailableComponents` representation and its membership
  queries.
- It exists so Init and Restore share one inventory value without depending on
  each other, and so neither owns a type the other must reach through.
- It reuses `weavelit-server-database`'s bounded `Name` and takes no other
  workspace path dependency.
- It does not own the runtime derivation of the inventory. The `weavelit-server`
  runtime derives the inventory once from the compiled-in module crates'
  identifier constants and supplies it as an inbound value, so this crate never
  depends on a Client, MFA, Service, or Log Module implementation.
- It does not own compatibility rules, workflow validation order, error
  presentation, or module registration.

## Asset Inventory

- `AGENTS.md`: Local component-inventory contract and boundary rules.
- `Cargo.toml`: Package metadata and its single Application Database contract
  dependency.
- `src/lib.rs`: The `AvailableComponents` value, its membership queries, and
  their unit tests.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root.
- Read the Server Architecture Design, Server Init Design, and Server Restore
  Design before changing what the inventory represents.
- Never restate a module identifier as a string literal in production code here
  or in a consumer; the runtime derivation reads each module crate's
  `MODULE_IDENTIFIER` so a compiled-in module and the inventory it is judged by
  cannot drift apart.
- Prefer the runtime's production inventory constructor in an end-to-end test.
  A test that supplies its own inventory proves only what that inventory
  contains, not what the shipped binary serves.
- Run the package tests during development and `make -C server check` before
  handoff.

## Standards and Conventions

- Update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the crate free of workflow, transport, persistence, and presentation
  behavior; it carries a value, not a policy.
- Keep every dependency exactly pinned with `default-features = false` and the
  minimum feature set the crate requires.
- Forbid `unsafe` code.
