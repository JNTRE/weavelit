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
- Keep Web UI Client Module documentation directly in this folder; move shared Client Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep Web UI application documentation in `../../clients/web-ui/` and provider-integration documentation in `../../service-modules/`.
