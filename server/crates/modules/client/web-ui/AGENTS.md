# Web UI Client Module Crate Agent Guide

This directory is reserved for the compiled-in Web UI Client Module crate. It
mounts browser-facing routes on the Server's HTTPS listener, uses secure
Server-managed browser sessions, and translates Web UI requests into the
Server's shared authorization flow.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Web UI Client Module's Server connection-surface behavior.
- It does not own the TypeScript and React Web UI application source; that belongs in `../../../../../web-ui/`.
- It does not own Server authorization policy, provider credentials, or provider integration behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Web UI Client Module crate-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/client-modules/web-ui/` and `../../../../../docs/clients/web-ui/` before changing Web UI access or connection behavior.
- Keep browser-facing request translation here and application presentation behavior in the Server Web UI source boundary.
- Add contract and security tests for route availability, sessions, caller identity, authorization, and sensitive-data exposure as required by `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Mount browser routes only on the configured Server HTTPS listener and make them unavailable when this Client Module is disabled.
- Derive Human User identity from the Server-managed session and pass every request to shared authorization.
- Never expose provider credentials, automation credentials, or internal error traces to the browser.