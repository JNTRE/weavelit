# Application Database Crates Agent Guide

This directory groups the Rust crates that define and implement the Weavelit
Server's internal Application Database contract. The Server core selects,
configures, and manages these compiled-in backends; they are separate from Log
Module destinations and do not operate as runtime plugins.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared Application Database contract and backend
  crate boundaries.
- It does not own Server business logic, backend selection, or Log Module storage and delivery behavior.
- Child paths own backend-specific driver, migration, transaction, health, and error behavior.

## Asset Inventory

- `weavelit-server-database/`: Backend-neutral Application Database contract crate, including Group public identity and administration, atomic TOTP enablement preview, and current Log configuration generation read contracts.
- `weavelit-server-database-authority/`: Server-owned capability key that gates persisted Audit Reference and opaque Audit terminal recovery decoding.
- `weavelit-server-database-sqlite/`: MVP SQLite Application Database backend crate boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../../docs/server/database/` for the shared backend contract and the matching backend guide before changing persistence behavior.
- MUST keep backend-specific concerns in their named child path and shared Server lifecycle behavior in the Server executable boundary.
- MUST add isolated integration tests for database, filesystem, configuration, serialization, or process behavior as required by `../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep Application Database backends compiled into the Server package and inaccessible as runtime-installable plugins.
- MUST keep this grouping directory free of a Cargo manifest; each contract or backend package belongs in its named child directory.
- MUST keep Application Database state separate from every Log Module destination.
- MUST preserve backend contract decisions in `../../../docs/server/database/` rather than duplicating them here.
