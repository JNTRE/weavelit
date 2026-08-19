# Server Init Agent Guide

This folder documents the Server-owned implementation boundary for **[Init](../../../glossary.md#states-and-requests)**. It applies the shared lifecycle contract to fresh application-state creation, request and secret handling, recovery-key delivery, System Log completion, and finalization.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Server Init contract and implementation design.
- Shared startup classification, database selection, workflow arbitration, and sealing remain in `../lifecycle-design.md`.
- User-visible Init workflow behavior remains in `../../user-stories/init-user-story.md`.
- No child directory currently defines a narrower documentation boundary.

## Asset Inventory

- `init-design.md`: Fresh-state Init contract, request and secret handling, initial recovery-key delivery, atomic state creation, System Log completion, and error-boundary design.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then `../../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST update `init-design.md` for Server-owned Init behavior.
- MUST update `../lifecycle-design.md` when a rule is shared with Restore.
- MUST update the Init user story when a contract change affects the user-visible workflow.

- MUST keep shared lifecycle rules in `../lifecycle-design.md`.
- MUST keep Restore-specific behavior in `../restore/`.
