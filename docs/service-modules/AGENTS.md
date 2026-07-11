# Service Modules Agent Guide

This folder documents the server-side **[Service Modules](../glossary.md#applications-and-interfaces)** that authenticate with named external services and implement supported Operations. It separates shared integration guidance from service-specific documentation, beginning with Zendesk.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared Service Module documentation and routing to service-specific module documentation.
- It does not own Client Module design or client-application behavior; those belong in the sibling `../client-modules/` and `../clients/` directories.
- The `zendesk/` child directory owns detailed documentation for the Zendesk Service Module.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Service Modules.
- `service-modules.md`: Placeholder for documentation shared by Service Modules.
- `zendesk/`: Documentation for the Zendesk Service Module.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep material shared by Service Modules in `service-modules.md`; place Zendesk-specific detail in `zendesk/`.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep client-connection design in `../client-modules/` and client-application behavior in `../clients/`.
