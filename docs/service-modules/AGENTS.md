# Service Modules Agent Guide

This folder documents the server-side **[Service Modules](../glossary.md#applications-and-interfaces)** that authenticate with named external services and implement supported Operations. It separates shared integration guidance from service-specific documentation, beginning with Zendesk.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns shared Service Module documentation and routing to service-specific module documentation.
- It does not own Client Module design or client-application behavior; those belong in the sibling `../client-modules/` and `../clients/` directories.
- The `zendesk/` child directory owns detailed documentation for the Zendesk Service Module.

## Asset Inventory

- `zendesk/`: Documentation for the Zendesk Service Module.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST place documentation shared by Service Modules directly in this folder; place Zendesk-specific detail in `zendesk/`.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep client-connection design in `../client-modules/` and client-application behavior in `../clients/`.
