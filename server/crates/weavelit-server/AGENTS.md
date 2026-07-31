# Weavelit Server Executable Agent Guide

This directory is reserved for the Weavelit Server executable crate. It will
assemble the trusted Server runtime that owns restricted pre-operational
startup, normal authenticated HTTPS API, authorization, authentication
configuration, provider integrations, and provider credentials while using the
adjacent compiled-in crates.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Server executable's composition and lifecycle behavior.
- It does not own individual Application Database backends or module implementations; those belong in sibling crate paths.
- It does not own Web UI source, Admin CLI behavior, tests, or packaging; those remain under their named Server paths.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server executable-boundary rules.
- `Cargo.toml`: Rust package manifest for the Weavelit Server executable crate.
- `src/`: Rust implementation source and executable entry point for the Weavelit Server.
- `tests/`: Crate-local integration tests for the Server executable.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the relevant canonical Server design under `../../../docs/server/` before changing lifecycle, Init, Restore, API, authentication, authorization, audit, database, or observability behavior.
- Keep provider-specific work in Service Module crates and client-facing request translation in Client Module crates.
- Add focused tests for changed behavior in the appropriate Server test boundary, following `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep startup-state classification and the lifecycle gate in the Server
  runtime composition. Persist and reconcile the deployment record and database
  locator through `weavelit-server-lifecycle`; never expose normal functions
  before the `Initialized` seal is durable or reopen Init or Restore as a
  fallback for missing or invalid deployment state.
- Preserve the Server's default-deny authorization and its ownership of final authorization decisions.
- Keep provider credentials and provider-integration behavior in the trusted Server environment; never move them into client applications.
- Keep canonical Server requirements in `../../../docs/` and update their owning document instead of restating them here.
