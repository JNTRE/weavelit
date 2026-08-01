# Server Init Agent Guide

This folder documents the Server-owned implementation boundary for **[Init](../../../glossary.md#states-and-requests)**. It applies the shared lifecycle contract to fresh application-state creation, request and secret handling, recovery-key delivery, System Log completion, and finalization.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Server Init contract and implementation design.
- Shared startup classification, database selection, workflow arbitration, and sealing remain in `../lifecycle-design.md`.
- User-visible Init workflow behavior remains in `../../user-stories/init-user-story.md`.
- No child directory currently defines a narrower documentation boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server Init design.
- `init-design.md`: Fresh-state Init contract, request and secret handling, initial recovery-key delivery, atomic state creation, System Log completion, and error-boundary design.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then `../../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Update `init-design.md` for Server-owned Init behavior.
- Update `../lifecycle-design.md` when a rule is shared with Restore.
- Update the Init user story when a contract change affects the user-visible workflow.
- Make minimal, targeted changes to the document that owns the changed behavior.
- Preserve existing document structure and filenames unless the task requires reorganization.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order.
- Keep this guide under 100 lines.
- Use exact canonical names from `../../../glossary.md`.
- On first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep shared lifecycle rules in `../lifecycle-design.md`.
- Keep Restore-specific behavior in `../restore/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links from `Related Documents`.
- Add canonical links required to reflect current source-of-truth references.
