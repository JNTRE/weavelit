# Clients Agent Guide

This folder documents Weavelit's client applications and future client adapters, routing component-specific work to their child folders. It keeps client behavior separate from the server-side Client Modules that provide their connection surfaces.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared client-application and client-adapter documentation and routing to individual client documentation.
- It does not own server-side Client Module design; that belongs in the sibling `../client-modules/` directory.
- The `mcp/`, `operations-cli/`, and `web-ui/` child directories own detailed documentation for their respective clients or adapters.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for client applications.
- `mcp/`: Future-only documentation boundary for the MCP adapter; it does not represent an implemented client.
- `operations-cli/`: Documentation for the **[Operations CLI](../glossary.md#applications-and-interfaces)** application.
- `web-ui/`: Documentation for the **[Web UI](../glossary.md#applications-and-interfaces)** application.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Place documentation shared by client applications and adapters directly in this folder; place component-specific detail in its child directory.
- Preserve the future-only status of `mcp/`; do not add implementation artifacts or describe it as currently supported.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep server-side Client Module design in `../client-modules/` and Service Module documentation in `../service-modules/`.
