# Admin CLI Crate Agent Guide

This directory is reserved for the Rust crate that implements the host-local
Admin CLI included with the Weavelit Server package. It provides Host
Administrators with Init and Server administration functions; it is not a
remote client interface and requires Unix `sudo` authority on the Server host.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns host-local Admin CLI behavior and its use of Server-owned administration logic.
- It does not own the remotely installed Weavelit CLI; that belongs in the dedicated client source tree.
- It does not own Web UI administration, Server policy, or individual module implementations.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Admin CLI crate-boundary rules.
- `Cargo.toml`: Rust package manifest for the Admin CLI executable crate.
- `src/`: Rust implementation source and executable entry point for the Admin CLI.
- `tests/`: Crate-local integration tests for the Admin CLI.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../docs/core-statements.md`, `../../../docs/security-model.md`, and the relevant Server design guide before changing Init or administration behavior.
- Keep interactive and non-interactive Init paths on the same Server-owned initialization logic.
- Add focused workflow and failure tests as required by `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep the Admin CLI host-local; do not expose its functions through remotely callable client interfaces.
- Do not accept sensitive non-interactive bootstrap values through environment variables or log or persist those values or their configuration.
- Preserve canonical Init, authentication, and administration requirements in `../../../docs/` rather than duplicating them here.
