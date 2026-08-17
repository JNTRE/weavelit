# SQLite Log Module Crate Agent Guide

This directory is reserved for the compiled-in Rust SQLite Log Module crate. It
persists pre-redacted structured System Logs, Audit Logs, or both to its own
SQLite destination without sharing Application Database persistence behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns SQLite-specific Log Module destination behavior.
- It does not own Application Database persistence, shared log semantics, redaction policy, or Log Module assignment decisions.
- Future child paths own only narrower SQLite Log Module guidance that differs from this crate boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and SQLite Log Module crate-boundary rules.
- `Cargo.toml`: Package manifest for the compiled-in SQLite Log Module.
- `src/`: SQLite destination implementation and isolated real-database tests.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/log-modules/`, `../../../../../docs/server/audit/`, and `../../../../../docs/server/observability/` before changing SQLite log delivery or storage behavior.
- Keep SQLite connection, migration, retention, and destination-health behavior in this crate and shared log semantics in their owning Server boundaries.
- Add isolated integration and security tests using real temporary SQLite destinations, following `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Accept only pre-redacted structured records; do not add secrets or unnecessary sensitive payloads to the SQLite destination.
- Keep this crate's files, schema, migrations, connections, health checks, configuration, and lifecycle separate from every Application Database backend.
- Keep the compiled-in `rusqlite` dependency workspace-pinned and reviewed before a version change.
