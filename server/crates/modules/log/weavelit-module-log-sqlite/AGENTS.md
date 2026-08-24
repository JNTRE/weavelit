# SQLite Log Module Crate Agent Guide

This directory is reserved for the compiled-in Rust SQLite Log Module crate. It
persists pre-redacted structured System Logs, Audit Logs, or both to its own
SQLite destination without sharing Application Database persistence behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns SQLite-specific Log Module destination behavior.
- It does not own Application Database persistence, shared log semantics, redaction policy, or Log Module assignment decisions.
- Future child paths own only narrower SQLite Log Module guidance that differs from this crate boundary.

## Asset Inventory

- `Cargo.toml`: Package manifest for the compiled-in SQLite Log Module.
- `src/`: SQLite destination implementation and isolated real-database tests.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/log-modules/`, `../../../../../docs/server/audit/`, and `../../../../../docs/server/observability/` before changing SQLite log delivery or storage behavior.
- MUST keep SQLite connection, migration, retention, and destination-health behavior in this crate and shared log semantics in their owning Server boundaries.
- MUST add isolated integration and security tests using real temporary SQLite destinations, following `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST accept only pre-redacted structured records; do not add secrets or unnecessary sensitive payloads to the SQLite destination.
- MUST keep this crate's files, schema, migrations, connections, health checks, configuration, and lifecycle separate from every Application Database backend.
- MUST keep the compiled-in `rusqlite` dependency workspace-pinned and reviewed before a version change.
