# Automation Identities Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** design for **[Automation Identities](../../glossary.md#identities-and-access)**, their responsible ownership, and accountability boundaries.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns implementation-design documentation for Automation Identity lifecycle, credential management, responsible-owner enforcement, and accountability integration.
- It does not own general authentication credential validation or authorization policy evaluation; those belong in the sibling `../authentication/` and `../authorization/` directories.
- Unsettled Automation Identity choices belong in `../../open-questions.md`.

## Asset Inventory

- `automation-identity-design.md`: Canonical implementation design for Automation Identities.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep Automation Identity design aligned with `../../security-model.md` and record only settled commitments in `../../spec.md`.
- MUST keep general credential validation in `../authentication/`, authorization policy evaluation in `../authorization/`, and Audit Log design in `../audit/`.

- MUST preserve the established active Responsible Owner and named Operation scope requirements; do not make an Automation Identity self-managing.
