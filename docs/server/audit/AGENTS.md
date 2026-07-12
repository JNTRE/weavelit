# Server Audit Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** **[Audit Log](../../glossary.md#applications-and-interfaces)** design for consequential **[Operations](../../glossary.md#applications-and-interfaces)**. It preserves accountability for the authenticated principal and, for automations, the **[Responsible Owner](../../glossary.md#identities-and-access)** without treating Audit Logs as operational observability data.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Audit Log design for **[Operation](../../glossary.md#applications-and-interfaces)** accountability, including the authenticated principal, result, and correlation identifier.
- It does not own operational diagnosis, metrics, or tracing; those belong in the sibling `../observability/` directory.
- Audit retention, redaction, and backup decisions that remain unsettled belong in `../../open-questions.md`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server Audit Logs.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep audit design aligned with `../../core-statements.md` and `../../security-model.md`; record unresolved retention and backup choices in `../../open-questions.md`.
- Keep operational diagnosis in `../observability/` and durable-data implementation detail in `../storage/`.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Preserve the accountability purpose of Audit Logs; do not substitute operational observability data for required audit evidence.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
