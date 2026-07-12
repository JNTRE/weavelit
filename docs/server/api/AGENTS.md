# Server API Agent Guide

This folder documents the authenticated HTTPS application interface of the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**. It will define the stable, versioned contract through which clients invoke supported **[Operations](../../glossary.md#applications-and-interfaces)** without duplicating service-specific behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server API contract design, including request, result, error, compatibility, pagination, and idempotency behavior.
- It does not own service-specific **[Operation](../../glossary.md#applications-and-interfaces)** semantics; those belong in `../../service-modules/`.
- API decisions that are not yet settled remain in `../../open-questions.md` until they can be recorded as a commitment.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Server API.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Add API contract detail only after the relevant wire-format or compatibility decision is settled; keep unresolved choices in `../../open-questions.md`.
- Keep service-specific **[Operation](../../glossary.md#applications-and-interfaces)** inputs and effects in `../../service-modules/`, and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Preserve the Server's API-first, authenticated, versioned application-interface commitments in `../../core-statements.md`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
