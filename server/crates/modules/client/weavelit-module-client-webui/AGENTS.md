# Web UI Client Module Crate Agent Guide

This directory is reserved for the compiled-in Web UI Client Module crate. It
mounts browser-facing routes on the Server's HTTPS listener, translates the
restricted Init and Restore contracts while the Server is uninitialized, and
uses secure Server-managed browser sessions and shared authorization during
normal operation.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Web UI Client Module's Server connection-surface behavior.
- It does not own the TypeScript and React Web UI application source; that belongs in `../../../../web-ui/`.
- It does not own lifecycle, Init, or Restore availability or semantics, Server
    authorization policy, provider credentials, or provider integration behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Web UI Client Module crate-boundary rules.
- `Cargo.toml`: Compiled-in Web UI Client Module package manifest.
- `build.rs`: Fail-closed check that the generated Web UI production assets embedded by this crate exist and are current before compilation.
- `build_manifest.rs`: Bundle-input inventory, SHA-256 hashing, and strict build content manifest verification used by `build.rs`.
- `src/lib.rs`: Pre-operational status request translation, Application Database selection request translation and its same-origin and CSRF trust gate, the compile-time Web UI asset allowlist and its delivery routes, and contract tests.
- `tests/build_manifest.rs`: Tests for the build-time asset freshness verification.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/server/lifecycle/lifecycle-design.md`, `../../../../../docs/server/lifecycle/init/init-design.md`, `../../../../../docs/server/lifecycle/restore/restore-design.md`, `../../../../../docs/client-modules/web-ui/`, and `../../../../../docs/clients/web-ui/` before changing Web UI pre-operational, access, or connection behavior.
- Keep browser-facing request translation here and application presentation behavior in the Server Web UI source boundary.
- Add contract and security tests for lifecycle-gated route availability, Init,
  Restore, sessions, caller identity, authorization, and sensitive-data
  exposure as required by `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Mount browser routes only on the configured Server HTTPS listener. Expose
    restricted Init and Restore routes only when the runtime lifecycle gate
    permits them; during normal operation, make this Client Module's routes
    unavailable when it is disabled.
- Translate shared status and database-selection requests only through
    Server-owned lifecycle operations, Init requests only through Init
    operations, and Restore requests only through Restore operations. Do not
    access the deployment record, database locator, secret files, backup
    plaintext, or Application Database directly or implement independent
    workflow validation.
- Derive Human User identity from the Server-managed session and pass every request to shared authorization.
- Never expose provider credentials, automation credentials, or internal error traces to the browser.
