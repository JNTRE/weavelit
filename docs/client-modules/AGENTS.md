# Client Modules Agent Guide

This folder documents the server-side **[Client Modules](../glossary.md#applications-and-interfaces)** that provide discrete client-facing connection surfaces to the Weavelit Server. It separates the Server's connection boundary from documentation for the client applications that use it.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared documentation for Client Modules and their role at the Weavelit Server boundary.
- It does not own Client CLI or Web UI application documentation; those belong in the sibling `../clients/` directory.
- A future module-specific child directory owns its detailed design and local guidance.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Client Modules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Place shared Client Module documentation directly in this folder; place module-specific detail in a child directory when one is introduced.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep client-application behavior in `../clients/` and Service Module documentation in `../service-modules/`.
