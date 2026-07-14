# Weavelit CLI Packaging Agent Guide

This directory is reserved for assets that package the separately released
Weavelit CLI. The release artifact installs the client without Rust, source
code, or provider credentials and remains independent of the Weavelit Server
package while compatible with its versioned application interface.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Weavelit CLI release-artifact boundaries.
- It does not own Weavelit CLI source, Server packaging, provider credentials, or Server initialization.
- Child paths own platform-specific packaging assets and installation behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Weavelit CLI packaging-boundary rules.
- `macos/`: macOS `arm64` Weavelit CLI packaging boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../docs/roadmap/milestone-8.md` and the Weavelit CLI requirements before changing release-artifact behavior.
- Keep packaging assets separate from source and verify installation against a versioned Server interface when release workflows are introduced.
- Record release build, installation, verification, and troubleshooting instructions with the packaged workflow.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Package the Weavelit CLI independently from the Server while respecting the versioned interface compatibility policy.
- Do not include Rust, source code, or provider credentials in an installed Weavelit CLI artifact.
- Keep platform-specific packaging behavior in its named child boundary.
