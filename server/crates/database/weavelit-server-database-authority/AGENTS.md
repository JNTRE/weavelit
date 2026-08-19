# Server Database Authority Crate Agent Guide

This crate supplies the capability key that distinguishes Server-owned
Application Database selection authority from an ordinary backend implementor.
It contains no persistence behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the `ServerDatabaseAuthority` capability type and nothing else.
- It does not own backend selection, persistence operations, or Audit Reference encoding.
- It has no child paths.

## Asset Inventory

- `Cargo.toml`: Package metadata; this crate takes no dependency.
- `src/lib.rs`: The `ServerDatabaseAuthority` capability type.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the repository root.
- MUST read the Application Database Design and Server Lifecycle Design before changing what this capability permits.
- MUST depend on this crate only from the database contract, lifecycle selection, and direct persistence test support that must issue or consume the decoder.
- Agents MUST NOT reexport this crate or `ServerDatabaseAuthority` from the database contract or lifecycle crate.

- MUST keep this crate free of dependencies, logic, and state so possession stays the only privilege it conveys.
- MUST keep `ServerDatabaseAuthority` privately represented and available only through an explicit dependency edge.
- MUST update this inventory whenever crate assets are added, removed, renamed, or moved.
