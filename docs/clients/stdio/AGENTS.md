# Standard Input/Output Client Adapter Agent Guide

This folder reserves the documentation boundary for the future standard input/output (stdio) client adapter. It captures the planned client-side adapter without implying that the stdio adapter is implemented or currently available.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns future design documentation for the standard input/output client adapter.
- It does not own standard input/output implementation artifacts or claims that the stdio adapter is currently available.
- Documentation shared by client applications and adapters belongs in the parent `../` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and future-scope rules for the standard input/output client adapter.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Do not add implementation artifacts or describe the stdio adapter as supported; add design documentation only after the relevant product decision is recorded in a canonical document.
- Keep shared client material in the parent `../` directory and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep current client documentation in sibling directories and future-only standard input/output material in this directory.
