# Log Module Crates Agent Guide

This directory is reserved for compiled-in Rust Log Module crates that receive
pre-redacted structured System Logs, Audit Logs, or both and persist or deliver
them to configured destinations. Log Modules remain separate from the Server's
Application Database and do not decide accountability or diagnostic semantics.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Log Module destination behavior for System Logs and Audit Logs.
- It does not own Application Database persistence, Audit Log accountability semantics, or System Log diagnostic semantics.
- Future child paths own destination-specific Log Module behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Log Module crate-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../docs/log-modules/`, `../../../../docs/server/audit/`, and `../../../../docs/server/observability/` before changing log delivery or storage behavior.
- Keep destination-specific persistence or delivery concerns here and shared Server log semantics in their owning Server boundary.
- Add isolated integration and security tests for destination failure and pre-redaction behavior, following `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Accept only pre-redacted structured records; do not add secrets or unnecessary sensitive payloads to a log destination.
- Keep Log Module destinations separate from Application Database persistence.
- Do not select retention, backup, purge, migration, or remote-delivery credential behavior without a recorded canonical decision.