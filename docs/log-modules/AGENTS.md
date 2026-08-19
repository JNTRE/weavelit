# Log Modules Agent Guide

This folder documents server-side **[Log Modules](../glossary.md#applications-and-interfaces)** that persist or deliver **[System Logs](../glossary.md#applications-and-interfaces)** and **[Audit Logs](../glossary.md#applications-and-interfaces)**. It separates log storage and delivery design from the Weavelit Server's application-state storage.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Log Module design, including log storage, delivery, retention, backup, restore, and migration boundaries.
- Log Modules receive records only after Server Audit or Observability completes the applicable pre-redaction boundary; they do not own sanitization of source records.
- It does not own the Server's **[Application Database](../glossary.md#applications-and-interfaces)** design; that belongs in `../server/database/`.
- It does not own Audit Log accountability or System Log operational-diagnosis semantics; those belong in `../server/audit/` and `../server/observability/`.

## Asset Inventory

- `audit-terminal-binding-retention-decision.md`: Accepted architecture decision for retained destination binding versions and constrained terminal supersession.
- `log-module-design.md`: Shared Log Module design, including Init configuration and log-type assignment.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep Log Module design aligned with the canonical logging policy in `../spec.md` and `../security-model.md`.
- MUST record only genuinely unresolved destination implementation choices in `../open-questions.md`; do not restate the settled destination recovery and retention policies there.

- MUST NOT select a retention duration, backup method, purge mechanism, or remote-delivery credential model without a recorded product or technical decision.
