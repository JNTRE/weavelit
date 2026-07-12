# Server Authentication Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** authentication design for human users and **[Automation Identities](../../glossary.md#identities-and-access)**. It applies the established **[Local Authentication](../../glossary.md#identities-and-access)** and **[External Authentication](../../glossary.md#identities-and-access)** commitments without defining implementation choices that remain unsettled.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server authentication design for local human accounts, **[Automation Identities](../../glossary.md#identities-and-access)**, and optional **[External Authentication](../../glossary.md#identities-and-access)**.
- It does not own authorization policy evaluation; that belongs in the sibling `../authorization/` directory.
- Authentication lifecycle and multifactor decisions that remain unsettled belong in `../../open-questions.md`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server authentication.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep authentication design aligned with `../../security-model.md` and record only settled commitments in `../../core-statements.md`.
- Keep authorization evaluation in `../authorization/` and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Preserve the established **[Local Authentication](../../glossary.md#identities-and-access)** default and optional **[External Authentication](../../glossary.md#identities-and-access)** boundary; do not add unsupported authentication methods as commitments.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
