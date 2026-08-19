# Server Tests Agent Guide

This directory is reserved for Server-focused integration and end-to-end tests.
It verifies observable Server behavior across the executable, compiled-in
modules, Web UI, persistence, and release workflows without moving tests into
the implementation crates they validate.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server-focused integration and end-to-end test suites.
- It does not replace focused unit, contract, or module tests that belong with their implementation boundaries.
- Future child paths own narrower test suites when their setup or validation differs from this directory's rules.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../docs/testing.md` and the implementation boundary's owning documentation before adding or changing a test workflow.
- MUST test public results, persisted state, audit events, and provider requests rather than private implementation call order.
- MUST keep tests deterministic, isolated, repeatable, and free of production services, live credentials, real user data, and network timing dependencies.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST cover both successful behavior and the most relevant failure or rejection condition for every tested workflow.
- MUST use controlled fakes or recorded fixtures for provider integrations; do not make live-provider access part of the default suite.
- MUST add deployment-like smoke coverage for Server package installation, Init, authenticated and denied requests, restart persistence, and clean shutdown when those workflows are introduced.
