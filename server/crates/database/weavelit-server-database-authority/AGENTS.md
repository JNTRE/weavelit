# Server Database Authority Crate Agent Guide

This crate supplies the capability key that distinguishes Server-owned
Application Database selection authority from an ordinary backend implementor.
It contains no persistence behavior.

## Purpose and Scope

- This directory owns the `ServerDatabaseAuthority` capability type and nothing else.
- It does not own backend selection, persistence operations, or Audit Reference encoding.
- It has no child paths.

## Asset Inventory

- `AGENTS.md`: Local routing, inventory, and capability-boundary rules.
- `Cargo.toml`: Package metadata; this crate takes no dependency.
- `src/lib.rs`: The `ServerDatabaseAuthority` capability type.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the repository root.
- Read the Application Database Design and Server Lifecycle Design before changing what this capability permits.
- Depend on this crate only from the database contract, lifecycle selection, and direct persistence test support that must issue or consume the decoder.
- Never reexport this crate or `ServerDatabaseAuthority` from the database contract or lifecycle crate.

## Standards and Conventions

- Keep this crate free of dependencies, logic, and state so possession stays the only privilege it conveys.
- Keep `ServerDatabaseAuthority` privately represented and available only through an explicit dependency edge.
- Update this inventory whenever crate assets are added, removed, renamed, or moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
