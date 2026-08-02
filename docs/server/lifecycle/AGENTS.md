# Server Lifecycle Agent Guide

This folder documents the shared pre-operational lifecycle boundary of the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**. It owns startup classification, deployment and database-selection persistence, workflow arbitration, mutation serialization, and sealing while routing workflow-specific contracts to focused child directories.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared Server lifecycle design used before normal authenticated operation.
- The `init/` and `restore/` child directories own the distinct application-state workflows that consume the shared lifecycle contract.
- Product commitments remain in `../../spec.md`.
- Cross-cutting security requirements remain in `../../security-model.md`.
- User-visible lifecycle narratives remain in `../user-stories/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server lifecycle design.
- `lifecycle-anchor-profile-decision.md`: Accepted architecture decision for trusted state-root configuration, anchor serialization and protection, at-rest key custody, and replay limits.
- `lifecycle-design.md`: Shared pre-operational startup classification, deployment-record and database-locator persistence, database selection, workflow arbitration, concurrency, and sealing design.
- `init/`: Server-owned fresh-state Init contract and implementation design.
- `restore/`: Server-owned Restore contract and implementation design.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Update `lifecycle-design.md` for behavior shared by Init and Restore.
- Update the appropriate child design for workflow-specific behavior.
- Keep user-visible lifecycle narratives in `../user-stories/`.
- Keep product and security commitments in their canonical top-level documents.
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
- Use exact canonical names from `../../glossary.md`.
- On first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep behavior shared by Init and Restore in `lifecycle-design.md`.
- Keep workflow-specific contracts in the appropriate child directory.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links from `Related Documents`.
- Add canonical links required to reflect current source-of-truth references.
