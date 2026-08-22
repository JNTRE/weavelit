# Weavelit CLI Client Module Agent Guide

This folder documents the server-side **[Weavelit CLI](../../glossary.md#applications-and-interfaces)** **[Client Module](../../glossary.md#applications-and-interfaces)**. It keeps the Server's connection-boundary detail separate from the Weavelit CLI application itself.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns documentation specific to the Weavelit CLI Client Module.
- It does not own Weavelit CLI application behavior; that belongs in `../../clients/weavelit-cli/`.
- Documentation shared by Client Modules belongs in the parent `../` directory.

## Asset Inventory


## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep Weavelit CLI Client Module documentation directly in this folder; move shared Client Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep Weavelit CLI application documentation in `../../clients/weavelit-cli/` and provider-integration documentation in `../../service-modules/`.
