# Weavelit Server Executable Agent Guide

This directory is reserved for the Weavelit Server executable crate. It will
assemble the trusted Server runtime that owns the authenticated HTTPS API,
authorization, authentication configuration, provider integrations, and
provider credentials while using the adjacent compiled-in crates.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Server executable's composition and lifecycle behavior.
- It does not own individual Application Database backends or module implementations; those belong in sibling crate paths.
- It does not own Web UI source, Admin CLI behavior, tests, or packaging; those remain under their named Server paths.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server executable-boundary rules.
- `tests/`: Crate-local integration tests for the Server executable.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the relevant canonical Server design under `../../../docs/server/` before changing API, authentication, authorization, audit, database, or observability behavior.
- Keep provider-specific work in Service Module crates and client-facing request translation in Client Module crates.
- Add focused tests for changed behavior in the appropriate Server test boundary, following `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Preserve the Server's default-deny authorization and its ownership of final authorization decisions.
- Keep provider credentials and provider-integration behavior in the trusted Server environment; never move them into client applications.
- Keep canonical Server requirements in `../../../docs/` and update their owning document instead of restating them here.
