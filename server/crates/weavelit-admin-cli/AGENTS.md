# Admin CLI Crate Agent Guide

This directory is reserved for the Rust crate that implements the host-local
Admin CLI included with the Weavelit Server package. It provides Host
Administrators with initialized-state local-account recovery when no application
session is available; it is not a remote client interface, does not expose Init
or Restore, and requires Unix `sudo` authority on the Server host.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns host-local Admin CLI behavior and its use of Server-owned local-account administration logic.
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
- Read `../../../docs/core-statements.md`, `../../../docs/security-model.md`, and the relevant Server design guide before changing administration behavior.
- Keep Init and Restore in the normal Server runtime and their capable Client
  Modules; do not add interactive, non-interactive, file-driven, or container
  Init or Restore paths to the Admin CLI.
- Add focused workflow and failure tests as required by `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the Admin CLI host-local; do not expose its functions through remotely callable client interfaces.
- Do not accept Init configuration, initialization secrets, backup artifacts,
  or private recovery keys through the Admin CLI, environment variables, or
  local files.
- Preserve canonical Init, authentication, and administration requirements in `../../../docs/` rather than duplicating them here.
