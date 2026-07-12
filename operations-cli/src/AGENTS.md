# Operations CLI Application Source Agent Guide

This directory is reserved for the Operations CLI application source. The CLI
runs on a user's local macOS system, authenticates to the Weavelit Server, and
submits only supported Operations through the Server's versioned HTTPS API; it
does not contain provider credentials, provider integrations, or administration.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Operations CLI application behavior and local client concerns.
- It does not own the Server-side Operations CLI Client Module, Server policy, provider integration, or provider credentials.
- Future child paths own only narrower application guidance that differs from this source boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Operations CLI source-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/clients/operations-cli/` for application requirements and `../../docs/client-modules/operations-cli/` for the Server connection boundary before changing behavior.
- Keep local client workflow and structured result handling here; leave identity derivation, authorization, and provider work with the Server.
- Add focused end-to-end or smoke tests for sign-in, sign-out, permitted Operation invocation, and expected client failure behavior, following `../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Use the configured Server HTTPS listener and `/api/v1/` routes for supported Operations; do not use Web UI browser routes.
- Do not add administrative commands, provider credentials, or provider-integration logic to the Operations CLI.
- Preserve Server-owned authorization decisions and canonical API requirements instead of duplicating them locally.