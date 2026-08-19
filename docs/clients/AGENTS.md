# Clients Agent Guide

This folder documents Weavelit's client applications and future client adapters, routing component-specific work to their child folders. It keeps client behavior separate from the server-side Client Modules that provide their connection surfaces.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns shared client-application and client-adapter documentation and routing to individual client documentation.
- It does not own server-side Client Module design; that belongs in the sibling `../client-modules/` directory.
- The `mcp/`, `weavelit-cli/`, and `web-ui/` child directories own detailed documentation for their respective clients or adapters.

## Asset Inventory

- `mcp/`: Future-only documentation boundary for the MCP adapter; it does not represent an implemented client.
- `weavelit-cli/`: Documentation for the **[Weavelit CLI](../glossary.md#applications-and-interfaces)** application.
- `web-ui/`: Documentation for the **[Web UI](../glossary.md#applications-and-interfaces)** application.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST place documentation shared by client applications and adapters directly in this folder; place component-specific detail in its child directory.
- MUST preserve the future-only status of `mcp/`; do not add implementation artifacts or describe it as currently supported.
- Use `../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep server-side Client Module design in `../client-modules/` and Service Module documentation in `../service-modules/`.
