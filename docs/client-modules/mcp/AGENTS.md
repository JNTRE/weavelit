# MCP Client Module Agent Guide

This folder reserves the documentation boundary for the future MCP **[Client Module](../../glossary.md#applications-and-interfaces)**. It captures the planned connection surface without implying that MCP support is implemented or currently available.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns future design documentation for the MCP Client Module.
- It does not own MCP implementation artifacts or claims that MCP support is currently available.
- Documentation shared by Client Modules belongs in the parent `../` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and future-scope rules for the MCP Client Module.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Do not add implementation artifacts or describe MCP as supported; add design documentation only after the relevant product decision is recorded in a canonical document.
- Keep shared Client Module material in the parent `../` directory and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep current Client Module documentation in sibling directories and future-only MCP material in this directory.
