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

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep Web UI-specific material directly in this folder; move shared client-application material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep server-side connection-surface documentation in `../../client-modules/` and provider-integration documentation in `../../service-modules/`.
