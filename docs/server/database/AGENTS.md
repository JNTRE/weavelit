# Application Database Agent Guide

This folder documents the internal Application Database boundary of the
Weavelit Server. It keeps shared persistence-contract design separate from the
Server core and routes backend-specific implementation detail to child folders.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns shared implementation-design documentation for the
  Server's **[Application Database](../../glossary.md#applications-and-interfaces)**
  backend contract.
- It does not own canonical product, security, or technical commitments; those
  remain in `../../spec.md`, `../../security-model.md`, and
  `../../open-questions.md`.
- The `sqlite/` child directory owns SQLite-backend-specific documentation;
  future backend directories own their respective implementation detail.

## Asset Inventory

- `application-database-design.md`: Shared Application Database backend contract and backup-and-Restore design.
- `sqlite/`: Documentation boundary for the MVP SQLite Application Database backend.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`,
  `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep shared backend-contract design in this folder and place database-specific
  implementation detail in the applicable child directory.
- MUST update `../../spec.md` for settled commitments and
  `../../open-questions.md` for unresolved choices instead of treating local
  design documentation as their replacement.

- Use exact canonical names from `../../glossary.md`; on first substantive use
  in a section, format a canonical term as a bold link to its glossary category.
- MUST keep backend-contract documentation here and database-specific driver,
  migration, transaction, connection-health, and error documentation in the
  relevant child backend directory.
