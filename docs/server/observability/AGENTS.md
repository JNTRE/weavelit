# Server Observability Agent Guide

This folder reserves the documentation boundary for future **[Weavelit Server](../../glossary.md#applications-and-interfaces)** observability design. It distinguishes operational diagnosis from audit accountability without implying that any logging, metrics, tracing, or monitoring design has been decided.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns future Server observability design documentation.
- It does not own implementation artifacts or claims that a logging, metrics, tracing, or monitoring design is currently defined.
- Audit accountability belongs in the sibling `../audit/` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and future-scope rules for Server observability.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Do not add implementation artifacts or observability design claims until the relevant decision is recorded in a canonical document.
- Keep audit accountability in `../audit/` and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep future observability material in this directory and do not restate audit-record requirements from `../audit/`.
