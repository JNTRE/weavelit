# Server Web UI Source Agent Guide

This directory is reserved for the TypeScript and React source of the Web UI
that is bundled into the Weavelit Server package. It is the browser application
for authenticated self-service and administration workflows, while the Web UI
Client Module owns its Server connection surface and final authorization stays
with the Server.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Web UI application source and its production asset bundle.
- It does not own browser route authentication or Server authorization policy; those belong in the Web UI Client Module and Server boundaries.
- It does not own a separately installed or released Web UI application.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server Web UI source-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/clients/web-ui/` for Web UI behavior and `../../docs/client-modules/web-ui/` for its Server connection boundary before changing a workflow.
- Keep presentation and client-side usability behavior here; rely on the Server for identity derivation and authorization decisions.
- Add focused end-to-end or smoke tests for user workflows and likely release failures, following `../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Build production Web UI assets as part of the Weavelit Server package; do not create a separate Web UI release.
- Do not treat client-side navigation or validation as authorization controls.
- Never expose provider credentials, automation credentials, or internal error traces in the browser.