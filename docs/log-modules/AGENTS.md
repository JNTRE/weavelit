# Log Modules Agent Guide

This folder documents server-side **[Log Modules](../glossary.md#applications-and-interfaces)** that persist or deliver **[System Logs](../glossary.md#applications-and-interfaces)** and **[Audit Logs](../glossary.md#applications-and-interfaces)**. It separates log storage and delivery design from the Weavelit Server's application-state storage.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Log Module design, including log storage, delivery, retention, backup, restore, and migration boundaries.
- Log Modules receive records only after Server Audit or Observability completes the applicable pre-redaction boundary; they do not own sanitization of source records.
- It does not own the Server's **[Application Database](../glossary.md#applications-and-interfaces)** design; that belongs in `../server/database/`.
- It does not own Audit Log accountability or System Log operational-diagnosis semantics; those belong in `../server/audit/` and `../server/observability/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Log Modules.
- `audit-terminal-binding-retention-decision.md`: Accepted architecture decision for retained destination binding versions and constrained terminal supersession.
- `log-module-design.md`: Shared Log Module design, including Init configuration and log-type assignment.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Keep Log Module design aligned with the canonical logging policy in `../spec.md` and `../security-model.md`.
- Record only genuinely unresolved destination implementation choices in `../open-questions.md`; do not restate the settled destination recovery and retention policies there.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Do not select a retention duration, backup method, purge mechanism, or remote-delivery credential model without a recorded product or technical decision.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
