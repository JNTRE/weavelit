# Server API Agent Guide

This folder documents the normal authenticated HTTPS application interface of
the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** and
shared API contract conventions. It will define the stable, versioned contract
through which clients invoke supported
**[Operations](../../glossary.md#applications-and-interfaces)** without
duplicating service-specific behavior. The restricted unauthenticated Init
and Restore lifecycle is owned by the
[Server Lifecycle Design](../lifecycle-design.md), with workflow semantics in
the [Server Init Design](../init-design.md) and
[Server Restore Design](../restore-design.md). Shared wire conventions used by
those contracts remain coordinated here.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server API contract design, including request, result, error, compatibility, pagination, and idempotency behavior.
- It does not own service-specific **[Operation](../../glossary.md#applications-and-interfaces)** semantics; those belong in `../../service-modules/`.
- It does not own pre-operational availability, database selection, or lifecycle
  gating; those belong in `../lifecycle-design.md`. Init recovery-key delivery
  belongs in `../init-design.md`, and Restore backup and private recovery-key
  handling belong in `../restore-design.md`.
- API decisions that are not yet settled remain in `../../open-questions.md` until they can be recorded as a commitment.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Server API.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Add API contract detail only after the relevant wire-format or compatibility decision is settled; keep unresolved choices in `../../open-questions.md`.
- Keep service-specific **[Operation](../../glossary.md#applications-and-interfaces)** inputs and effects in `../../service-modules/`, and use `../../glossary.md` for canonical terminology.
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
- Preserve the Server's API-first, versioned interface, including the restricted
  Init and Restore exceptions and normal authenticated-operation commitments in
  `../../core-statements.md`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
