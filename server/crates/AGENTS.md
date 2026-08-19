# Server Crates Agent Guide

This directory is the Rust source boundary for the Weavelit Server package. It
groups core orchestration, authentication, Application Database, and compiled-in
Client, MFA, Log, and Service Module crates that are released together with the
Server.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Server package's Rust crate layout.
- It does not own Web UI source, Server integration tests, or Debian packaging; those remain sibling Server paths.
- Child paths own executable, database, and module-specific implementation guidance.

## Asset Inventory

- `core/`: Server runtime, lifecycle, Init, Restore, and authentication crates.
- `database/`: Internal Application Database contract and backend crates.
- `modules/`: Compiled-in Client, MFA, Log, and Service Module crate boundaries.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.
- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- MUST read the matching canonical guide under `../../docs/server/`, `../../docs/client-modules/`, `../../docs/mfa-modules/`, `../../docs/log-modules/`, or `../../docs/service-modules/` before changing a crate's behavior.
- MUST keep each responsibility in its named child crate rather than adding cross-cutting behavior to this directory.
- MUST add or update focused tests with implementation behavior changes as required by `../../docs/testing.md`.
- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep all Rust code in this directory on the Rust 1.97 stable toolchain required by `../../docs/testing.md`.
- MUST keep grouping directories free of Cargo manifests; each package belongs in a child directory named for its Cargo package.
- MUST keep Server package components as compiled-in crates; do not create runtime-installable module plugins.
- MUST preserve the canonical module and database boundaries in `../../docs/` rather than duplicating their decisions here.
