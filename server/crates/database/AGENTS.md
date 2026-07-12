# Application Database Crates Agent Guide

This directory is reserved for Rust crates that implement the Weavelit Server's
internal Application Database backend contract. The Server core selects,
configures, and manages these compiled-in backends; they are separate from Log
Module destinations and do not operate as runtime plugins.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Application Database backend crate boundaries.
- It does not own Server business logic, backend selection, or Log Module storage and delivery behavior.
- Child paths own backend-specific driver, migration, transaction, health, and error behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Application Database crate-boundary rules.
- `sqlite/`: MVP SQLite Application Database backend crate boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../docs/server/database/` for the shared backend contract and the matching backend guide before changing persistence behavior.
- Keep backend-specific concerns in their named child path and shared Server lifecycle behavior in the Server executable boundary.
- Add isolated integration tests for database, filesystem, configuration, serialization, or process behavior as required by `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Keep Application Database backends compiled into the Server package and inaccessible as runtime-installable plugins.
- Keep Application Database state separate from every Log Module destination.
- Preserve backend contract decisions in `../../../docs/server/database/` rather than duplicating them here.