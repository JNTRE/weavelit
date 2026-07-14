# Log Modules Agent Guide

This folder documents server-side **[Log Modules](../glossary.md#applications-and-interfaces)** that persist or deliver **[System Logs](../glossary.md#applications-and-interfaces)** and **[Audit Logs](../glossary.md#applications-and-interfaces)**. It separates log storage and delivery design from the Weavelit Server's application-state storage.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Log Module design, including log storage, delivery, retention, backup, restore, and migration boundaries.
- It does not own Server application-state storage; its design remains in `../open-questions.md` until a dedicated documentation boundary is warranted.
- It does not own Audit Log accountability or System Log operational-diagnosis semantics; those belong in `../server/audit/` and `../server/observability/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Log Modules.
- `spec.md`: Shared Log Module design, including Init configuration and log-type
  assignment.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep Log Module design aligned with the canonical logging policy in `../core-statements.md` and `../security-model.md`.
- Record unresolved destination backup, restore, migration, retention-bound, purge-execution, and remote-credential choices in `../open-questions.md`.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Do not select a retention duration, backup method, purge mechanism, or remote-delivery credential model without a recorded product or technical decision.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
