# Weavelit Server Integration Tests Agent Guide

This directory owns crate-local integration tests for the Weavelit Server
executable. Cargo compiles each Rust test file here as an external consumer of
the executable crate.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns focused integration tests for Server executable behavior.
- It does not replace unit tests colocated with pure implementation logic.
- It does not own cross-package, release, Web UI, or full Server workflow tests;
  those belong in `../../../../tests/`.

## Asset Inventory

- `audit_generation_resolver_authority.rs`: External compile fixture proving the inert Audit configuration-generation resolver remains Server-private.
- `fixtures/forbidden-audit-generation-resolver/`: Standalone consumer that must fail when it imports the private resolver boundary.
- `startup.rs`: Restricted startup, lifecycle composition, and process behavior integration tests.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  `../../../AGENTS.md`, `../../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the owning Server design and `../../../../../docs/testing.md` before adding
  or changing a test.
- MUST test observable executable behavior through public interfaces with isolated
  temporary resources; do not assert private implementation call order.
- MUST keep tests deterministic, repeatable, and free of production services, live
  credentials, real user data, and network timing dependencies.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST cover the expected result and the most relevant rejection or failure condition
  for each test workflow.
