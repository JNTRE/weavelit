# Server Observability Agent Guide

This folder documents **[System Log](../../glossary.md#applications-and-interfaces)** design and reserves the remaining **[Weavelit Server](../../glossary.md#applications-and-interfaces)** observability boundary. It distinguishes operational diagnosis from Audit Log accountability without implying that metrics, tracing, monitoring, or alerting design has been decided.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server System Log design and future observability design documentation.
- It does not own implementation artifacts or claims that metrics, tracing, monitoring, or alerting design is currently defined.
- Audit accountability belongs in the sibling `../audit/` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and future-scope rules for Server observability.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep System Log design aligned with the canonical logging policy. Do not add implementation artifacts or metrics, tracing, monitoring, or alerting claims until the relevant decision is recorded in a canonical document.
- Keep audit accountability in `../audit/` and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep future observability material in this directory and do not restate Audit Log requirements from `../audit/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
