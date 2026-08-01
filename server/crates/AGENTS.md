# Server Crates Agent Guide

This directory is the Rust source boundary for the Weavelit Server package. It
groups the Server executable with the internal Application Database backend and
the compiled-in Client, MFA, Log, and Service Module crates that are released
together with the Server.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Server package's Rust crate layout.
- It does not own Web UI source, Server integration tests, or Debian packaging; those remain sibling Server paths.
- Child paths own executable, database, and module-specific implementation guidance.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Rust crate-boundary rules.
- `database/`: Internal Application Database backend crates; `database/sqlite/` owns the MVP backend implementation.
- `modules/`: Compiled-in Client, MFA, Log, and Service Module crate boundaries.
- `weavelit-server/`: Weavelit Server executable crate.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Read the matching canonical guide under `../../docs/server/`, `../../docs/client-modules/`, `../../docs/mfa-modules/`, `../../docs/log-modules/`, or `../../docs/service-modules/` before changing a crate's behavior.
- Keep each responsibility in its named child crate rather than adding cross-cutting behavior to this directory.
- Add or update focused tests with implementation behavior changes as required by `../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep all Rust code in this directory on the Rust 1.97 stable toolchain required by `../../docs/testing.md`.
- Keep Server package components as compiled-in crates; do not create runtime-installable module plugins.
- Preserve the canonical module and database boundaries in `../../docs/` rather than duplicating their decisions here.
