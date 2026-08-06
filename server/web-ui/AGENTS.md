# Server Web UI Source Agent Guide

This directory is reserved for the TypeScript and React source of the Web UI
that is bundled into the Weavelit Server package. It is the browser application
for restricted Init and Restore and authenticated self-service and
administration workflows, while the Web UI Client Module owns its Server
connection surface and final lifecycle and authorization enforcement stays with
the Server.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Web UI application source and its production asset bundle.
- It does not own browser route authentication or Server authorization policy; those belong in the Web UI Client Module and Server boundaries.
- It does not own a separately installed or released Web UI application.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server Web UI source-boundary rules.
- `.node-version`: Pinned Node.js release used by local development, the development container, and CI.
- `.npmrc`: npm client settings that force exact version pinning and engine enforcement.
- `index.html`: Vite entry document for the single-page application.
- `package.json`: Web UI manifest, exact dependency pins, and build, test, and validation scripts.
- `package-lock.json`: Fully resolved npm dependency lock for reproducible installs.
- `scripts/`: Build-output validation scripts run by the Server quality gate.
- `src/`: TypeScript and React application source and its unit tests.
- `tsconfig.json`: TypeScript compiler configuration for the application and its tests.
- `vite.config.ts`: Vite build, deterministic output-naming, and Vitest configuration.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/clients/web-ui/` for Web UI behavior and `../../docs/client-modules/web-ui/` for its Server connection boundary before changing a workflow.
- Keep presentation and client-side usability behavior here; rely on the Server
  for lifecycle, Init and Restore availability, identity derivation, and
  authorization decisions.
- Add focused end-to-end or smoke tests for user workflows and likely release failures, following `../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Build production Web UI assets as part of the Weavelit Server package; do not create a separate Web UI release.
- Do not treat client-side navigation or validation as authorization controls.
- Never expose provider credentials, automation credentials, or internal error traces in the browser.
