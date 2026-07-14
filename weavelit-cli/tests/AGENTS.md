# Weavelit CLI Tests Agent Guide

This directory is reserved for focused Weavelit CLI tests. It verifies the
installed client's user-facing behavior against the versioned Server interface,
including authentication, permitted Operation invocation, structured results,
and the failure conditions most likely to make a release unusable.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Weavelit CLI-focused test suites.
- It does not replace Server contract, authorization, provider integration, or package tests in their owning boundaries.
- Future child paths own narrower test suites when their setup or validation differs from this directory's rules.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Weavelit CLI test-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/testing.md` and the relevant Weavelit CLI requirement before adding or changing a test workflow.
- Exercise observable CLI results and Server interaction boundaries rather than private implementation call order.
- Keep tests deterministic, isolated, repeatable, and free of live provider credentials, provider services, real user data, and network timing dependencies.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Cover sign-in, sign-out, permitted invocation, denied or unavailable access, and structured result behavior as each workflow is implemented.
- Verify the separately packaged CLI against the versioned Server interface on its supported macOS `arm64` platform when release workflows are introduced.
- Use controlled Server fixtures; do not depend on a live provider as part of the default CLI test suite.
