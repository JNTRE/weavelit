# Server Storage Agent Guide

This folder documents the durable-data design of the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**. It will cover the Server state needed for policy, audit records, idempotency, authentication, schedules, and provider connection state without preselecting a storage technology or backup model.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server durable-data, retention, backup, and restore design.
- It does not own audit-record accountability semantics; those belong in the sibling `../audit/` directory.
- Storage technology, data retention, redaction, and backup decisions that remain unsettled belong in `../../open-questions.md`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server storage.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Add storage design detail only after the relevant technology, retention, or backup decision is settled; keep unresolved choices in `../../open-questions.md`.
- Keep audit-accountability design in `../audit/` and use `../../glossary.md` for canonical terminology.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Do not select a storage technology, retention duration, redaction policy, or backup method without a recorded product or technical decision.
