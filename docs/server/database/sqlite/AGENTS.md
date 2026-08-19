# SQLite Application Database Agent Guide

This folder documents the MVP SQLite implementation of the Weavelit Server's
internal Application Database backend. It isolates SQLite-specific behavior
from the shared backend contract and from Server business logic.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns SQLite-specific implementation-design documentation for
  the **[Application Database](../../../glossary.md#applications-and-interfaces)**
  backend.
- It does not own the shared Application Database backend contract, Server
  business logic, Log Module storage, or canonical product and security
  commitments.
- Future child paths own narrower SQLite design boundaries only when their
  guidance differs from this directory's rules.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the SQLite Application Database backend.
- `sqlite-application-database-design.md`: SQLite driver, migration, transaction, connection-health, error, and test design.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`,
  `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Keep SQLite-specific driver, schema migration, transaction, connection-health,
  and error behavior in this folder.
- Update shared backend-contract documentation in `../` and canonical decisions
  in `../../../spec.md` or `../../../open-questions.md` rather than
  duplicating them here.
- Make minimal, targeted changes and update this inventory when assets are
  added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../../glossary.md`; on first substantive
  use in a section, format a canonical term as a bold link to its glossary
  category.
- Keep SQLite-specific documentation consistent with the shared Application
  Database backend contract; do not present SQLite behavior as a rule for every
  backend.
