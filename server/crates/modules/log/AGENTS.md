# Log Module Crates Agent Guide

This directory is reserved for compiled-in Rust Log Module crates that receive
pre-redacted structured System Logs, Audit Logs, or both and persist or deliver
them to configured destinations. Log Modules remain separate from the Server's
Application Database and do not decide accountability or diagnostic semantics.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Log Module destination behavior for System Logs and Audit Logs.
- It does not own Application Database persistence, Audit Log accountability semantics, or System Log diagnostic semantics.
- Future child paths own destination-specific Log Module behavior.

## Asset Inventory

- `weavelit-module-log-sqlite/`: SQLite Log Module crate boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../docs/log-modules/`, `../../../../docs/server/audit/`, and `../../../../docs/server/observability/` before changing log delivery or storage behavior.
- MUST keep destination-specific persistence or delivery concerns here and shared Server log semantics in their owning Server boundary.
- MUST add isolated integration and security tests for destination failure and pre-redaction behavior, following `../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST accept only pre-redacted structured records; do not add secrets or unnecessary sensitive payloads to a log destination.
- MUST keep Log Module destinations separate from Application Database persistence.
- Agents MUST NOT select retention, backup, purge, migration, or remote-delivery credential behavior without a recorded canonical decision.
