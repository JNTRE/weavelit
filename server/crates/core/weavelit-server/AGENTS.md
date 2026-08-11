# Weavelit Server Executable Agent Guide

This crate assembles the trusted restricted lifecycle startup runtime, composes
the SQLite backend catalog, and classifies startup state before any capability
is exposed. It also owns the Server-core local authentication route layer
(login, session validation, and logout) and the live authorization composition
that evaluates every User Plane and Administration Plane request once a
deployment is sealed. MFA and provider integrations remain deferred to later
epics.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own,
and where child paths own detailed rules.

- This directory owns the Server executable's restricted startup composition,
  SQLite backend factory, state-root configuration reading, startup classification
  dispatch, and stable error presentation.
- It does not own individual Application Database backends, lifecycle domain,
  module implementations, Web UI source, or packaging.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server executable-boundary rules.
- `Cargo.toml`: Rust package manifest for the Weavelit Server executable crate.
- `src/lib.rs`: Restricted lifecycle startup composition, SQLite backend factory,
  state-root configuration reading, `classify_restricted_startup`, the listener's
  serving-mode switch, and stable error presentation.
- `src/authentication.rs`: The local login, session-validation, and logout route
  decisions: account and password-verifier resolution, the equal-work denial
  path, the single-permit login admission lane, session issuance and
  revocation, and best-effort authentication-failure System Log dispatch before
  a denial is returned.
- `src/authorization.rs`: `AuthorizationRuntime`, the live composition point for
  both authorization decisions: the compiled-in served-component inventory, the
  catalog built from one live component-enablement read, the `ValidatedSession`
  gate that makes skipping session validation a compile failure, the live,
  uncached reads of the account's grants and component enablement on every
  call, and best-effort authorization-denial System Log dispatch before a
  denial is returned.
- `src/operational.rs`: The single operational composition seam: the shared
  Application Database handle a sealed workflow hands over, the operational
  composer that mounts the Web UI operational surface and the authentication
  routes together with their transport registrations, and the mounted surface
  value the serving-mode switch accepts.
- `src/restore.rs`: Server-owned Restore orchestration that joins backup
  validation to the lifecycle typestate chain, the one-time ticket store and
  admission registrations behind the two-step submission protocol, the System
  Log acknowledgement delivery, and in-process activation of normal operation.
- `src/transport.rs`: Route-registered transport profiles and the admission
  typestate that orders rate limiting, classification, framing, pre-body
  validation, and permit acquisition ahead of any body allocation.
- `src/main.rs`: Thin executable entry point that reads state-root configuration
  and calls the library composition function.
- `tests/startup.rs`: Composition and process-level tests for restricted startup
  covering fresh start, restart persistence, selection, pending states, and
  fail-closed failure categories.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the relevant canonical Server design under `../../../../docs/server/` before changing lifecycle, Init, Restore, API, authentication, authorization, audit, database, or observability behavior.
- Keep provider-specific work in Service Module crates and client-facing request translation in Client Module crates.
- Add focused tests for changed behavior in the appropriate Server test boundary, following `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep startup-state classification and the lifecycle gate in the Server
  runtime composition. Persist and validate the deployment record and database
  locator through `weavelit-server-lifecycle`; never expose normal functions
  before the `Initialized` seal is durable or reopen Init or Restore as a
  fallback for missing or invalid deployment state.
- Preserve the Server's default-deny authorization and its ownership of final authorization decisions.
- Keep provider credentials and provider-integration behavior in the trusted Server environment; never move them into client applications.
- Keep canonical Server requirements in `../../../../docs/` and update their owning document instead of restating them here.
