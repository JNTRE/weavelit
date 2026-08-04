# Server Core Crates Agent Guide

This directory groups the Weavelit Server runtime and its core lifecycle, Init,
and Restore orchestration crates. These crates own Server-wide composition and
pre-operational application workflows rather than component-specific storage or
module behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server runtime and core orchestration crate boundaries.
- It does not own Application Database backends or Client, MFA, Log, and Service Module implementations.
- Child paths own executable, lifecycle, Init, and Restore implementation guidance.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server core crate-boundary rules.
- `weavelit-server/`: Weavelit Server executable crate.
- `weavelit-server-log/`: Typed Log Module contract and compiled-in catalog.
- `weavelit-server-lifecycle/`: Backend-neutral lifecycle domain, validation, and runtime-supplied Application Database catalog contract.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest child `AGENTS.md`, then this `AGENTS.md`, `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the matching canonical design under `../../../docs/server/` before changing runtime, lifecycle, Init, or Restore behavior.
- Keep component-specific persistence and module behavior in their sibling grouping directories.
- Add or update focused tests with implementation behavior changes as required by `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep this grouping directory free of a Cargo manifest; each package belongs in a child directory named for its Cargo package.
- Keep runtime and pre-operational orchestration separate from component-specific persistence and module implementations.
