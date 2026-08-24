# Weavelit CLI Agent Guide

This folder documents the **[Weavelit CLI](../../glossary.md#applications-and-interfaces)**, Weavelit's separately packaged command-line application for **[User Plane](../../glossary.md#applications-and-interfaces)** and **[Administration Plane](../../glossary.md#applications-and-interfaces)** functions. It is the dedicated home for Weavelit CLI detail within the broader client-application documentation boundary.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns documentation specific to the Weavelit CLI application.
- It does not own shared client-application documentation; that belongs in the parent `../` directory.
- It does not own the server-side Client Module that provides the Weavelit CLI connection surface; that belongs in `../../client-modules/`.

## Asset Inventory


## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep Weavelit CLI-specific material directly in this folder; move shared client-application material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep server-side connection-surface documentation in `../../client-modules/` and provider-integration documentation in `../../service-modules/`.
