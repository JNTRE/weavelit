# SQLite Application Database Agent Guide

This folder documents the MVP SQLite implementation of the Weavelit Server's
internal Application Database backend. It isolates SQLite-specific behavior
from the shared backend contract and from Server business logic.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns SQLite-specific implementation-design documentation for
  the **[Application Database](../../../glossary.md#applications-and-interfaces)**
  backend.
- It does not own the shared Application Database backend contract, Server
  business logic, Log Module storage, or canonical product and security
  commitments.
- Future child paths own narrower SQLite design boundaries only when their
  guidance differs from this directory's rules.

## Asset Inventory

- `sqlite-application-database-design.md`: SQLite driver, migration, transaction, connection-health, error, and test design.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then `../../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`,
  `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the [Documentation Standards](../../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep SQLite-specific driver, schema migration, transaction, connection-health,
  and error behavior in this folder.
- MUST update shared backend-contract documentation in `../` and canonical decisions
  in `../../../spec.md` or `../../../open-questions.md` rather than
  duplicating them here.

- Use exact canonical names from `../../../glossary.md`; on first substantive
  use in a section, format a canonical term as a bold link to its glossary
  category.
- MUST keep SQLite-specific documentation consistent with the shared Application
  Database backend contract; do not present SQLite behavior as a rule for every
  backend.
