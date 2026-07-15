# Weavelit Server Integration Tests Agent Guide

This directory owns crate-local integration tests for the Weavelit Server
executable. Cargo compiles each Rust test file here as an external consumer of
the executable crate.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns focused integration tests for Server executable behavior.
- It does not replace unit tests colocated with pure implementation logic.
- It does not own cross-package, release, Web UI, or full Server workflow tests;
  those belong in `../../../tests/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing and integration-test boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the owning Server design and `../../../../docs/testing.md` before adding
  or changing a test.
- Test observable executable behavior through public interfaces with isolated
  temporary resources; do not assert private implementation call order.
- Keep tests deterministic, repeatable, and free of production services, live
  credentials, real user data, and network timing dependencies.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Cover the expected result and the most relevant rejection or failure condition
  for each test workflow.
