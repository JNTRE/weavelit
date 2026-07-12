# SQLite Application Database Crate Agent Guide

This directory is reserved for the Rust crate that implements the MVP SQLite
Application Database backend. It owns SQLite-specific driver integration,
schema migrations, transaction behavior, connection health handling, and
backend-specific errors behind the Server's internal backend contract.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns SQLite-specific Application Database backend behavior.
- It does not own the shared backend contract, Server business logic, or Log Module storage.
- Future child paths own only narrower SQLite guidance that differs from this backend boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and SQLite backend crate-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../../docs/server/database/sqlite/` and `../../../../docs/server/database/` before changing SQLite persistence behavior.
- Keep SQLite-specific driver, migration, transaction, connection-health, and error behavior in this crate.
- Add isolated integration tests using temporary resources for persistence behavior, following `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Do not present SQLite behavior as a requirement for every Application Database backend.
- Keep SQLite persistence separate from the default SQLite Log Module database and destination.
- Preserve shared backend decisions in `../../../../docs/server/database/` rather than duplicating them here.