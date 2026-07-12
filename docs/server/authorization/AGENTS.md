# Server Authorization Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** authorization design that makes the final default-deny decision for each requested **[Operation](../../glossary.md#applications-and-interfaces)**. It separates permission and policy evaluation from caller authentication and service-specific behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server authorization design for named **[Operation](../../glossary.md#applications-and-interfaces)** permissions and policy evaluation.
- It does not own authentication credential validation; that belongs in the sibling `../authentication/` directory.
- Detailed Group assignment-evaluation and additional-role decisions that remain
  unsettled belong in `../../open-questions.md`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server authorization.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep authorization design aligned with `../../security-model.md` and record only settled commitments in `../../core-statements.md`.
- Keep credential validation in `../authentication/` and service-specific **[Operation](../../glossary.md#applications-and-interfaces)** behavior in `../../service-modules/`.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Preserve the Server's final, default-deny, per-**[Operation](../../glossary.md#applications-and-interfaces)** authorization boundary; do not grant broad provider-integration access.
