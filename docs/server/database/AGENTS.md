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
  remain in `../../core-statements.md`, `../../security-model.md`, and
  `../../open-questions.md`.
- The `sqlite/` child directory owns SQLite-backend-specific documentation;
  future backend directories own their respective implementation detail.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for
  the Application Database.
- `sqlite/`: Documentation boundary for the MVP SQLite Application Database
  backend.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`,
  `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Keep shared backend-contract design in this folder and place database-specific
  implementation detail in the applicable child directory.
- Update `../../core-statements.md` for settled commitments and
  `../../open-questions.md` for unresolved choices instead of treating local
  design documentation as their replacement.
- Make minimal, targeted changes and update this inventory when assets are
  added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use
  in a section, format a canonical term as a bold link to its glossary category.
- Keep backend-contract documentation here and database-specific driver,
  migration, transaction, connection-health, and error documentation in the
  relevant child backend directory.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.