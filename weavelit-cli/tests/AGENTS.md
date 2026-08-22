# Weavelit CLI Tests Agent Guide

This directory is reserved for focused Weavelit CLI tests. It verifies the
installed client's user-facing behavior against the versioned Server interface,
including authentication, User Plane and Administration Plane requests,
structured results, and the failure conditions most likely to make a release
unusable.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Weavelit CLI-focused test suites.
- It does not replace Server contract, authorization, provider integration, or package tests in their owning boundaries.
- Future child paths own narrower test suites when their setup or validation differs from this directory's rules.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../docs/testing.md` and the relevant Weavelit CLI requirement before adding or changing a test workflow.
- MUST exercise observable CLI results and Server interaction boundaries rather than private implementation call order.
- MUST keep tests deterministic, isolated, repeatable, and free of live provider credentials, provider services, real user data, and network timing dependencies.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST cover sign-in, sign-out, permitted and denied User Plane and Administration Plane requests, unavailable access, and structured result behavior as each workflow is implemented.
- MUST verify the separately packaged CLI against the versioned Server interface on its supported macOS `arm64` platform when release workflows are introduced.
- MUST use controlled Server fixtures; do not depend on a live provider as part of the default CLI test suite.
