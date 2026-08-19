# Zendesk Service Module Agent Guide

This folder documents the Zendesk **[Service Module](../../glossary.md#applications-and-interfaces)**, including its supported Operations and service-specific design. It is the dedicated provider-integration boundary beneath the shared Service Module documentation.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns documentation specific to the Zendesk Service Module.
- It does not own documentation shared by Service Modules; that belongs in the parent `../` directory.
- It does not own Client Module design or client-application behavior; those belong in `../../client-modules/` and `../../clients/`.

## Asset Inventory


## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep Zendesk-specific material directly in this folder; move shared Service Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep shared provider-integration guidance in the parent `../` directory and client documentation in `../../clients/`.
