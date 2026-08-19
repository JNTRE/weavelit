# Client Modules Agent Guide

This folder documents the server-side **[Client Modules](../glossary.md#applications-and-interfaces)** that provide discrete client-facing connection surfaces to the Weavelit Server. It separates the Server's connection boundary from documentation for the client applications that use it.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns shared documentation for Client Modules and their function at the Weavelit Server boundary.
- It does not own Weavelit CLI or Web UI application documentation; those belong in the sibling `../clients/` directory.
- The `mcp/`, `weavelit-cli/`, and `web-ui/` child directories own detailed documentation for their respective Client Modules.

## Asset Inventory

- `mcp/`: Future-only documentation boundary for the MCP Client Module; it does not represent an implemented interface.
- `weavelit-cli/`: Documentation for the server-side **[Weavelit CLI](../glossary.md#applications-and-interfaces)** Client Module.
- `web-ui/`: Documentation for the server-side Client Module that provides the **[Web UI](../glossary.md#applications-and-interfaces)** connection surface.

## Working Rules

- Before editing, MUST read the nearest `AGENTS.md`, then each parent guide upward to the repository root.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST keep shared Client Module documentation in this directory and module-specific detail in its child directory.
- MUST preserve the future-only status of `mcp/`; do not add implementation artifacts or describe it as currently supported.
- MUST keep client-application behavior in `../clients/` and Service Module documentation in `../service-modules/`.
