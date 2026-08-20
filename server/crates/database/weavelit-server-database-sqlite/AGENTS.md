# SQLite Application Database Crate Agent Guide

This directory contains the Rust crate that implements the MVP SQLite
Application Database backend. It owns SQLite-specific driver integration,
schema migrations, transaction behavior, connection health handling, and
backend-specific errors behind the Server's internal backend contract.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns SQLite-specific Application Database backend behavior.
- It does not own the shared backend contract, Server business logic, or Log Module storage.
- Future child paths own only narrower SQLite guidance that differs from this backend boundary.

## Asset Inventory

- `Cargo.toml`: Package manifest and exact production and test dependencies.
- `migrations/`: Immutable embedded SQL migrations for the SQLite schema.
- `src/`: Trusted-path connection setup, migrations, state inspection, atomic checkpoint and completion operations, application-state reading and writing, live session storage, MFA replay watermark storage, immutable Log Module configuration-generation persistence with current and historical reads, private Audit terminal recovery storage, and private error mapping.
- `tests/`: Public-boundary connection, migration, inspection, checkpoint, application-state, live-session, MFA replay watermark, immutable Log Module configuration-generation seeding, reads, rollback, immutability, and fail-closed integrity tests, plus recovery-exclusion tests using isolated real SQLite files.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../../../docs/server/database/sqlite/` and `../../../../docs/server/database/` before changing SQLite persistence behavior.
- MUST keep SQLite-specific driver, migration, transaction, connection-health, and error behavior in this crate.
- MUST add isolated integration tests using temporary resources for persistence behavior, following `../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Agents MUST NOT present SQLite behavior as a requirement for every Application Database backend.
- MUST keep SQLite persistence separate from the default SQLite Log Module database and destination.
- MUST preserve shared backend decisions in `../../../../docs/server/database/` rather than duplicating them here.
