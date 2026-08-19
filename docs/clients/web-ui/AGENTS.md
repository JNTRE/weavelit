# Web UI Agent Guide

This folder documents the **[Web UI](../../glossary.md#applications-and-interfaces)**, Weavelit's browser-based administrative client. It is the dedicated home for Web UI detail within the broader client-application documentation boundary.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns documentation specific to the Web UI application.
- It does not own shared client-application documentation; that belongs in the parent `../` directory.
- It does not own the server-side Client Module that provides the Web UI connection surface; that belongs in `../../client-modules/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Web UI.
- `web-ui-application-design.md`: Build toolchain, generated production outputs, application shell, status presentation states, the first-launch Init and Restore choice, the Application Database selection control, the Init workflow, the Restore submission control, and the sign-in control for the Web UI browser application.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Keep first-launch Init and Restore presentation and client-side usability
  behavior here; keep their Server connection contract in
  `../../client-modules/web-ui/`, shared lifecycle authority in
  `../../server/lifecycle/lifecycle-design.md`, and workflow authority in
  `../../server/lifecycle/init/init-design.md` and `../../server/lifecycle/restore/restore-design.md`.
- Keep Web UI-specific material directly in this folder; move shared client-application material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep server-side connection-surface documentation in `../../client-modules/` and provider-integration documentation in `../../service-modules/`.
