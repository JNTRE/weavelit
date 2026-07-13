# Admin CLI Integration Tests Agent Guide

This directory owns crate-local integration tests for the host-local Admin CLI.
Cargo compiles each Rust test file here as an external consumer of the CLI
crate.

## Purpose and Scope

- This directory owns focused integration tests for Admin CLI behavior.
- It does not replace unit tests colocated with pure implementation logic.
- It does not own cross-package, release, or full Server workflow tests; those
  belong in `../../../tests/`.

## Asset Inventory

- `AGENTS.md`: Local routing and integration-test boundary rules.

## Usage Guidance

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the owning administration design and `../../../../docs/testing.md` before
  adding or changing a test.
- Test observable CLI output, exit status, and resulting state with isolated
  temporary resources; do not assert private implementation call order.
- Keep tests deterministic, repeatable, and free of production services, live
  credentials, real user data, and network timing dependencies.

## Standards and Conventions

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Cover successful behavior and the most relevant safe failure condition for each
  test workflow, including absence of sensitive values in output and errors.