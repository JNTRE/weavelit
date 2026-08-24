# MCP Client Adapter Agent Guide

This folder reserves the documentation boundary for the future MCP client adapter. It captures the planned client-side adapter without implying that MCP support is implemented or currently available.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns future design documentation for the MCP client adapter.
- It does not own MCP implementation artifacts or claims that MCP support is currently available.
- Documentation shared by client applications and adapters belongs in the parent `../` directory.

## Asset Inventory


## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST NOT add implementation artifacts or describe MCP as supported; add design documentation only after the relevant product decision is recorded in a canonical document.
- MUST keep shared client material in the parent `../` directory and use `../../glossary.md` for canonical terminology.

- MUST keep current client documentation in sibling directories and future-only MCP material in this directory.
