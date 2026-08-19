# Server Audit Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** **[Audit Log](../../glossary.md#applications-and-interfaces)** design for consequential authenticated application actions, including **[Operations](../../glossary.md#applications-and-interfaces)**. It preserves accountability for the authenticated principal, direct Attempt-to-outcome linkage, and, for automations, the **[Responsible Owner](../../glossary.md#identities-and-access)** without treating Audit Logs as operational observability data.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Audit Log construction and pre-redaction design for authenticated application accountability, including the authenticated principal, action or **[Operation](../../glossary.md#applications-and-interfaces)**, target, phase-bound result, direct Attempt linkage, and correlation identifier, before records reach a Log Module.
- It does not own Init or Restore lifecycle-result logging; that belongs in the sibling `../observability/` directory.
- It does not own operational diagnosis, metrics, or tracing; those belong in the sibling `../observability/` directory.
- Audit destination retention and backup decisions that remain unsettled belong in `../../open-questions.md`; required pre-redaction follows `../../security-model.md`.
- The canonical Audit Log record schema, event classification taxonomy, and SQLite migration compatibility are defined in [Log Module Design](../../log-modules/log-module-design.md); this guide does not duplicate that schema.

## Asset Inventory

- `audit-log-design.md`: Canonical Server Audit producer, bounded record, redaction, taxonomy, and delivery design.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep audit design aligned with `../../spec.md` and `../../security-model.md`; record unresolved retention and backup choices in `../../open-questions.md`.
- MUST keep operational diagnosis in `../observability/` and Audit Log storage and delivery design in `../../log-modules/`.

- MUST preserve the accountability purpose of Audit Logs; do not substitute operational observability data for required audit evidence.
