# Web UI Client Module Crate Agent Guide

This directory is reserved for the compiled-in Web UI Client Module crate. It
declares the browser-facing capabilities the Server mounts on its HTTPS
listener, owns the compile-time Web UI asset allowlist and its delivery, and
uses secure Server-managed browser sessions and shared authorization during
normal operation. The shared request schemas, validation, handlers, and fixed
response profile behind its declared capabilities live in
`../weavelit-module-client/`.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Web UI Client Module's browser-specific Server connection-surface behavior and its declared capabilities.
- It does not own the shared Client Module API contract, request schemas, validation, handlers, or fixed response profile; those belong in `../weavelit-module-client/`.
- It does not own the TypeScript and React Web UI application source; that belongs in `../../../../web-ui/`.
- It does not own lifecycle, Init, or Restore availability or semantics, Server
    authorization policy, provider credentials, or provider integration behavior.

## Asset Inventory

- `Cargo.toml`: Compiled-in Web UI Client Module package manifest.
- `build.rs`: Fail-closed check that the generated Web UI production assets embedded by this crate exist and are current before compilation.
- `build_manifest.rs`: Bundle-input inventory, SHA-256 hashing, and strict build content manifest verification used by `build.rs`.
- `src/lib.rs`: The pre-operational and operational capability declarations this Client Module returns, including authenticated account reads, the compile-time Web UI asset allowlist and its delivery routes, and asset contract tests.
- `tests/build_manifest.rs`: Tests for the build-time asset freshness verification.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/server/lifecycle/lifecycle-design.md`, `../../../../../docs/server/lifecycle/init/init-design.md`, `../../../../../docs/server/lifecycle/restore/restore-design.md`, `../../../../../docs/client-modules/web-ui/`, and `../../../../../docs/clients/web-ui/` before changing Web UI pre-operational, access, or connection behavior.
- MUST keep browser-specific request translation here and application presentation behavior in the Server Web UI source boundary.
- MUST keep behavior every Client Module must share in `../weavelit-module-client/` rather than reimplementing it here.
- MUST add contract and security tests for lifecycle-gated route availability, Init,
  Restore, sessions, caller identity, authorization, and sensitive-data
  exposure as required by `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST mount browser routes only on the configured Server HTTPS listener. Expose
    restricted Init and Restore routes only when the runtime lifecycle gate
    permits them; during normal operation, make this Client Module's routes
    unavailable when it is disabled.
- MUST declare a capability only by supplying its collaborators, so this Client
    Module can neither claim a capability it did not supply nor supply one it
    did not claim.
- MUST translate shared status and database-selection requests only through
    Server-owned lifecycle operations, Init requests only through Init
    operations, and Restore requests only through Restore operations. Do not
    access the deployment record, database locator, secret files, backup
    plaintext, or Application Database directly or implement independent
    workflow validation.
- MUST derive Human User identity from the Server-managed session and pass every request to shared authorization.
- Agents MUST NOT expose provider credentials, automation credentials, or internal error traces to the browser.
