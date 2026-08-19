# Server Observability Agent Guide

This folder documents **[System Log](../../glossary.md#applications-and-interfaces)** design and reserves the remaining **[Weavelit Server](../../glossary.md#applications-and-interfaces)** observability boundary. It distinguishes operational diagnosis from Audit Log accountability without implying that metrics, tracing, monitoring, or alerting design has been decided.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server System Log construction, classification, and pre-redaction design before records reach a Log Module, including durable Init and Restore completion results, plus future observability design documentation.
- It does not own implementation artifacts or claims that metrics, tracing, monitoring, or alerting design is currently defined.
- Audit accountability belongs in the sibling `../audit/` directory.
- The canonical System Log record schema and event classification taxonomy are defined in [Log Module Design](../../log-modules/log-module-design.md); this guide does not duplicate that schema.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and future-scope rules for Server observability.
- `authentication-failure-record-design.md`: Fixed classification, detail, and
  delivery-timing design for the local authentication-failure System Log
  record.
- `authorization-denial-record-design.md`: Fixed classification, detail, and
  delivery-timing design for the authorization-denial System Log record.
- `audit-log-unavailability-record-design.md`: Typed safe context, delivery
  timing, terminal-recovery reporting, and stable consequential-operation
  rejection for an unavailable Audit Log destination.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Keep System Log design aligned with the canonical logging and pre-redaction policy in `../../spec.md` and `../../security-model.md`. Do not add implementation artifacts or metrics, tracing, monitoring, or alerting claims until the relevant decision is recorded in a canonical document.
- Keep audit accountability in `../audit/` and use `../../glossary.md` for canonical terminology.
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
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep future observability material in this directory and do not restate Audit Log requirements from `../audit/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
