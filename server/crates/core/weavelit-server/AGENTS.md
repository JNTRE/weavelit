# Weavelit Server Executable Agent Guide

This crate assembles the trusted restricted lifecycle startup runtime, composes
the SQLite backend catalog, and classifies startup state before any capability
is exposed. It composes the Init and Restore orchestrations that join their
validation crates to the lifecycle typestate chain, gate each workflow's route
mounting on lifecycle eligibility and, for Init finalization, on a written
recovery-key response, and activate normal operation in-process. It also owns
the Server-core local authentication route layer (login, session validation,
logout, TOTP second-factor verification, and TOTP enrollment) and the live
authorization composition that evaluates every User Plane and Administration
Plane request once a deployment is sealed. Provider integrations remain
deferred to later epics.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Server executable's restricted startup composition,
  SQLite backend factory, state-root configuration reading, startup classification
  dispatch, and stable error presentation.
- It does not own individual Application Database backends, lifecycle domain,
  module implementations, Web UI source, or packaging.

## Asset Inventory

- `Cargo.toml`: Rust package manifest for the Weavelit Server executable crate.
- `src/lib.rs`: Restricted lifecycle startup composition, SQLite backend factory,
  state-root configuration reading, `classify_restricted_startup`, the listener's
  serving-mode switch, the listener-owned response-write acknowledgement, and
  stable error presentation.
- `src/authentication.rs`: The local login, session-validation, and logout route
  decisions: account and password-verifier resolution, the equal-work denial
  path, the single-permit login admission lane, session issuance and
  revocation, and best-effort authentication-failure System Log dispatch before
  a denial is returned. It also owns the TOTP second-factor and enrollment
  decisions: the eight-row login admission truth table, the single-use
  continuation ticket, second-factor code verification, enrollment opening from
  both a login continuation and a live session, enrollment confirmation, and
  the enrolled-account preview and session-revoking Module enablement
  primitives.
- `src/authorization.rs`: `AuthorizationRuntime`, the live composition point for
  both authorization decisions: the compiled-in served-component inventory, the
  catalog built from one live component-enablement read, the `ValidatedSession`
  gate that makes skipping session validation a compile failure, atomic binding
  of a successful Administration Plane proof to that session's actor, exact
  digest, and Client Module, the live uncached reads of the account's grants and
  component enablement on every call, and best-effort authorization-denial
  System Log dispatch before a denial is returned.
- `src/init.rs`: Server-owned Init orchestration and its two-request submission
  protocol: the delivery stage that prepares the recovery-key checkpoint and
  releases the lifecycle mutex, database handle, and mutation-lane permit
  before the key is saved, the post-write publication that mounts finalization
  only after the key response is actually written, reauthorization and
  proof-of-possession verification, Log Module assignment preflight, the one
  blocking commit-through-activation chain, and the asymmetric
  actionable-versus-fail-closed failure handling.
- `src/operational.rs`: The single operational composition seam: the shared
  Application Database handle a sealed workflow hands over, the operational
  composer that mounts the Web UI operational surface and the authentication
  routes together with their transport registrations, activates bounded Audit
  terminal recovery, exposes the internal pre-consequential drain gate, and
  builds the mounted surface value the serving-mode switch accepts.
- `src/operational_audit.rs`: Trusted exact-generation Audit destination
  resolution and the process-serialized, bounded active-then-late terminal
  recovery drains that run at activation and before consequential mutations.
- `src/operational_logging.rs`: Normal-operation support that best-effort
  records typed Audit Log destination and terminal-recovery failures in the
  System Log and maps only pre-mutation delivery failures to the stable
  payload-free consequential-operation rejection.
- `src/restore.rs`: Server-owned Restore orchestration that joins backup
  validation to the lifecycle typestate chain, the one-time ticket store and
  admission registrations behind the two-step submission protocol, the System
  Log acknowledgement delivery, and in-process activation of normal operation.
- `src/transport.rs`: Route-registered transport profiles and the admission
  typestate that orders rate limiting, classification, framing, pre-body
  validation, and permit acquisition ahead of any body allocation.
- `src/main.rs`: Thin executable entry point that reads state-root configuration
  and calls the library composition function.
- `tests/audit_generation_resolver_authority.rs`: Compile-fail boundary proving the active Audit configuration-generation resolver is not a public construction surface.
- `tests/startup.rs`: Composition and process-level tests for restricted startup
  covering fresh start, restart persistence, selection, pending states, and
  fail-closed failure categories.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.
- Update this inventory when assets in this directory are added, removed, renamed, or moved.
- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the relevant canonical Server design under `../../../../docs/server/` before changing lifecycle, Init, Restore, API, authentication, authorization, audit, database, or observability behavior.
- MUST keep provider-specific work in Service Module crates and client-facing request translation in Client Module crates.
- MUST add focused tests for changed behavior in the appropriate Server test boundary, following `../../../../docs/testing.md`.
- MUST keep startup-state classification and the lifecycle gate in the Server
  runtime composition. Persist and validate the deployment record and database
  locator through `weavelit-server-lifecycle`; never expose normal functions
  before the `Initialized` seal is durable or reopen Init or Restore as a
  fallback for missing or invalid deployment state.
- MUST preserve the Server's default-deny authorization and its ownership of final authorization decisions.
- MUST keep provider credentials and provider-integration behavior in the trusted Server environment; never move them into client applications.
