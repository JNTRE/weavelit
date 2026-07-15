# Server Tests Agent Guide

This directory is reserved for Server-focused integration and end-to-end tests.
It verifies observable Server behavior across the executable, compiled-in
modules, Web UI, persistence, and release workflows without moving tests into
the implementation crates they validate.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server-focused integration and end-to-end test suites.
- It does not replace focused unit, contract, or module tests that belong with their implementation boundaries.
- Future child paths own narrower test suites when their setup or validation differs from this directory's rules.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server test-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/testing.md` and the implementation boundary's owning specification before adding or changing a test workflow.
- Test public results, persisted state, audit events, and provider requests rather than private implementation call order.
- Keep tests deterministic, isolated, repeatable, and free of production services, live credentials, real user data, and network timing dependencies.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Cover both successful behavior and the most relevant failure or rejection condition for every tested workflow.
- Use controlled fakes or recorded fixtures for provider integrations; do not make live-provider access part of the default suite.
- Add deployment-like smoke coverage for Server package installation, Init, authenticated and denied requests, restart persistence, and clean shutdown when those workflows are introduced.