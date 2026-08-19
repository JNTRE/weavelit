# Application Database Agent Guide

This folder documents the internal Application Database boundary of the
Weavelit Server. It keeps shared persistence-contract design separate from the
Server core and routes backend-specific implementation detail to child folders.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared implementation-design documentation for the
  Server's **[Application Database](../../glossary.md#applications-and-interfaces)**
  backend contract.
- It does not own canonical product, security, or technical commitments; those
  remain in `../../spec.md`, `../../security-model.md`, and
  `../../open-questions.md`.
- The `sqlite/` child directory owns SQLite-backend-specific documentation;
  future backend directories own their respective implementation detail.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Application Database.
- `application-database-design.md`: Shared Application Database backend contract and backup-and-Restore design.
- `sqlite/`: Documentation boundary for the MVP SQLite Application Database backend.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`,
  `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Keep shared backend-contract design in this folder and place database-specific
  implementation detail in the applicable child directory.
- Update `../../spec.md` for settled commitments and
  `../../open-questions.md` for unresolved choices instead of treating local
  design documentation as their replacement.
- Make minimal, targeted changes and update this inventory when assets are
  added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use
  in a section, format a canonical term as a bold link to its glossary category.
- Keep backend-contract documentation here and database-specific driver,
  migration, transaction, connection-health, and error documentation in the
  relevant child backend directory.
