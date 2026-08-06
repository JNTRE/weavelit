# Weavelit Server Source Agent Guide

This directory implements the Ubuntu-packaged Weavelit Server: its executable,
compiled-in modules, Web UI source, tests, and Debian packaging. It is the
trusted application boundary that owns the HTTPS API, policy enforcement,
provider integrations, and provider credentials.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns source and release assets for the Weavelit Server package.
- It does not own the separately packaged Weavelit CLI application; that belongs in the dedicated client source tree.
- Component-specific implementation guidance belongs in the matching canonical documentation under `../docs/` until a child source directory gains distinct local workflow rules.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and source-boundary rules for the Weavelit Server.
- `.dockerignore`: Development container build-context exclusions.
- `Cargo.lock`: Resolved Rust dependencies for reproducible Server workspace builds.
- `Cargo.toml`: Rust workspace manifest for Server package crates.
- `containers/`: Development and production Containerfiles for the Server.
- `crates/`: Rust crate locations for the Server, Application Database contract and backends, and compiled-in modules.
- `Makefile`: Standard local and CI entry point for the Server Web UI and Rust quality gates.
- `packaging/`: Release packaging assets for the Server package; `packaging/deb/` owns Debian-specific files.
- `rust-toolchain.toml`: Pinned Rust toolchain and required quality-gate components.
- `tests/`: Server-focused integration and end-to-end tests.
- `web-ui/`: TypeScript and React source whose production assets are bundled into the Server package.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Before changing a component, read its matching guide under `../docs/server/`, `../docs/client-modules/`, `../docs/mfa-modules/`, `../docs/log-modules/`, or `../docs/service-modules/`.
- Make minimal, targeted changes and preserve the existing ownership boundaries between Server crates, Web UI, tests, and packaging.
- Update the owning documentation and focused tests with each implementation behavior change, as required by `../docs/testing.md`.
- Run `make check` for the complete Server Rust quality-gate suite.
- Add a child `AGENTS.md` only when that path develops distinct commands, validation, security constraints, or documentation routing.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the Server, Application Database backend, and compiled-in Client, MFA, Log, and Service Modules under `crates/`.
- Keep Web UI source under `web-ui/`; do not create a separately released Web UI application.
- Keep Server release packaging under `packaging/` and Server-focused integration or end-to-end tests under `tests/`.
- Preserve the canonical boundaries in `../docs/`; link to or update those documents instead of duplicating their decisions here.
