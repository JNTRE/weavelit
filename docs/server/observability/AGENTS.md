# Server Observability Agent Guide

This folder documents **[System Log](../../glossary.md#applications-and-interfaces)** design and reserves the remaining **[Weavelit Server](../../glossary.md#applications-and-interfaces)** observability boundary. It distinguishes operational diagnosis from Audit Log accountability without implying that metrics, tracing, monitoring, or alerting design has been decided.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server System Log construction, classification, and pre-redaction design before records reach a Log Module, including durable Init and Restore completion results, plus future observability design documentation.
- It does not own implementation artifacts or claims that metrics, tracing, monitoring, or alerting design is currently defined.
- Audit accountability belongs in the sibling `../audit/` directory.
- The canonical System Log record schema and event classification taxonomy are defined in [Log Module Design](../../log-modules/log-module-design.md); this guide does not duplicate that schema.

## Asset Inventory

- `authentication-failure-record-design.md`: Fixed classification, detail, and
  delivery-timing design for the local authentication-failure System Log
  record.
- `authorization-denial-record-design.md`: Fixed classification, detail, and
  delivery-timing design for the authorization-denial System Log record.
- `audit-log-unavailability-record-design.md`: Typed safe context, delivery
  timing, and stable consequential-operation rejection for an unavailable
  Audit Log destination.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep System Log design aligned with the canonical logging and pre-redaction policy in `../../spec.md` and `../../security-model.md`. Do not add implementation artifacts or metrics, tracing, monitoring, or alerting claims until the relevant decision is recorded in a canonical document.
- MUST keep audit accountability in `../audit/` and use `../../glossary.md` for canonical terminology.

- MUST keep future observability material in this directory and do not restate Audit Log requirements from `../audit/`.
