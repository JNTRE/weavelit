# Client Modules Agent Guide

This folder documents the server-side **[Client Modules](../glossary.md#applications-and-interfaces)** that provide discrete client-facing connection surfaces to the Weavelit Server. It separates the Server's connection boundary from documentation for the client applications that use it.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared documentation for Client Modules and their function at the Weavelit Server boundary.
- It does not own Weavelit CLI or Web UI application documentation; those belong in the sibling `../clients/` directory.
- The `mcp/`, `operations-cli/`, and `web-ui/` child directories own detailed documentation for their respective Client Modules.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Client Modules.
- `mcp/`: Future-only documentation boundary for the MCP Client Module; it does not represent an implemented interface.
- `operations-cli/`: Documentation for the server-side Client Module that provides the **[Weavelit CLI](../glossary.md#applications-and-interfaces)** connection surface.
- `web-ui/`: Documentation for the server-side Client Module that provides the **[Web UI](../glossary.md#applications-and-interfaces)** connection surface.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Place shared Client Module documentation directly in this folder; place module-specific detail in its appropriate child directory.
- Preserve the future-only status of `mcp/`; do not add implementation artifacts or describe it as currently supported.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep client-application behavior in `../clients/` and Service Module documentation in `../service-modules/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
