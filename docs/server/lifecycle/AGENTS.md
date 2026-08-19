# Server Lifecycle Agent Guide

This folder documents the shared pre-operational lifecycle boundary of the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**. It owns startup classification, deployment and database-selection persistence, workflow arbitration, mutation serialization, and sealing while routing workflow-specific contracts to focused child directories.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared Server lifecycle design used before normal authenticated operation.
- The `init/` and `restore/` child directories own the distinct application-state workflows that consume the shared lifecycle contract.
- Product commitments remain in `../../spec.md`.
- Cross-cutting security requirements remain in `../../security-model.md`.
- User-visible lifecycle narratives remain in `../user-stories/`.

## Asset Inventory

- `lifecycle-anchor-profile-decision.md`: Accepted architecture decision for trusted state-root configuration, anchor serialization and protection, at-rest key custody, and replay limits.
- `lifecycle-design.md`: Shared pre-operational startup classification, deployment-record and database-locator persistence, database selection, workflow arbitration, concurrency, and sealing design.
- `init/`: Server-owned fresh-state Init contract and implementation design.
- `restore/`: Server-owned Restore contract and implementation design.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST update `lifecycle-design.md` for behavior shared by Init and Restore.
- MUST update the appropriate child design for workflow-specific behavior.
- MUST keep user-visible lifecycle narratives in `../user-stories/`.
- MUST keep product and security commitments in their canonical top-level documents.

- MUST keep behavior shared by Init and Restore in `lifecycle-design.md`.
- MUST keep workflow-specific contracts in the appropriate child directory.
