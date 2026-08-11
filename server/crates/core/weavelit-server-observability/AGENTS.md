# Server Observability Crate Agent Guide

This crate is Server Observability: the only producer of complete, pre-redacted
System Log records. It currently produces the Restore completion result and the
local authentication-failure result; it is the long-term home for
Server-produced operational telemetry.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns System Log record construction, classification selection, and pre-redaction of Server-produced events.
- It does not own Audit records, record delivery, destination configuration, workflow orchestration, or Application Database access.
- It has no child paths.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Observability boundary rules.
- `Cargo.toml`: Package metadata and its path dependencies on the log contract and Application Database contract.
- `src/lib.rs`: The `ServerObservability` producer and its stable error type.
- `src/restore.rs`: The Restore completion result and its paired persisted obligation.
- `src/authentication.rs`: The fixed local authentication-failure System Log
  result, carrying only a fresh record identifier, the event time, the
  response's correlation identifier, and a constant classification and detail.
- `tests/`: Behavior tests for completion construction, field binding, rejection, and redaction.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../../docs/log-modules/log-module-design.md` and `../../../../docs/spec.md` before changing what an event records.
- Give each event family its own module so this crate stays navigable as monitoring grows.
- Build a record and its persisted obligation together from the same fields so a post-commit record cannot drift from the committed obligation.
- Add or update focused tests with implementation behavior changes as required by `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Every field must be pre-redacted and bounded before construction; never place a recovery key, backup content, restored identity, credential, or filesystem path in a record.
- Keep error variants free of event content so a rendered error cannot disclose what failed validation.
- Obtain the record issuer from `ServerLogAuthority`; never widen the log contract's private constructors.
- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
