# Client Module Crates Agent Guide

This directory is reserved for compiled-in Client Module crates that provide
client-facing connection surfaces to the Weavelit Server. A Client Module
authenticates and translates its client's requests into the shared Operation
contract; the Server remains the final authorization authority.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared Client Module crate layout.
- It does not own client-application behavior, Server authorization policy, or Service Module provider integrations.
- Child paths own connection-surface behavior for their named client.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Client Module crate-boundary rules.
- `operations-cli/`: Operations CLI Client Module crate boundary.
- `web-ui/`: Web UI Client Module crate boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the matching guide under `../../../../docs/client-modules/` before changing a Client Module connection surface.
- Read `../../../../docs/clients/` when a change affects the corresponding client application's behavior.
- Add contract and security tests for changed accepted requests, stable responses, identity derivation, and denied access as required by `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Derive caller identity from Server-validated credentials or sessions; never trust identity, group, or permission claims supplied by a client.
- Pass every accepted request to the shared Server authorization policy.
- Keep client-application behavior in its named application boundary rather than duplicating it in a Client Module crate.