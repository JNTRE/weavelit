# Zendesk Service Module Agent Guide

This folder documents the Zendesk **[Service Module](../../glossary.md#applications-and-interfaces)**, including its supported Operations and service-specific design. It is the dedicated provider-integration boundary beneath the shared Service Module documentation.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns documentation specific to the Zendesk Service Module.
- It does not own documentation shared by Service Modules; that belongs in the parent `../` directory.
- It does not own Client Module design or client-application behavior; those belong in `../../client-modules/` and `../../clients/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Zendesk Service Module.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep Zendesk-specific material directly in this folder; move shared Service Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep shared provider-integration guidance in the parent `../` directory and client documentation in `../../clients/`.
