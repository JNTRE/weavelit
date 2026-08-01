# Web UI Client Module Agent Guide

This folder documents the server-side **[Client Module](../../glossary.md#applications-and-interfaces)** that provides the **[Web UI](../../glossary.md#applications-and-interfaces)** connection surface to the Weavelit Server. It keeps the Server's connection-boundary detail separate from the Web UI application itself.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns documentation specific to the Web UI Client Module.
- It does not own Web UI application behavior; that belongs in `../../clients/web-ui/`.
- Documentation shared by Client Modules belongs in the parent `../` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Web UI Client Module.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Keep the Web UI Client Module's Init-capable and Restore-capable transport
  behavior here. `../../server/lifecycle/lifecycle-design.md` owns availability and
  database selection, `../../server/lifecycle/init/init-design.md` owns fresh-state secrets and
  recovery-key delivery, and `../../server/lifecycle/restore/restore-design.md` owns backup and
  private recovery-key handling.
- Keep Web UI Client Module documentation directly in this folder; move shared Client Module material to the parent `../` directory.
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
- Keep Web UI application documentation in `../../clients/web-ui/` and provider-integration documentation in `../../service-modules/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
