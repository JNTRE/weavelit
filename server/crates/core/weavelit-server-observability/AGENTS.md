# Server Observability Crate Agent Guide

This crate is Server Observability: the only producer of complete, pre-redacted
System Log records. It currently produces the Restore and Init completion
results, local authentication and authorization denials, and Audit Log
destination unavailability; it is the long-term home for Server-produced
operational telemetry.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns System Log record construction, classification selection, and pre-redaction of Server-produced events.
- It does not own Audit records, record delivery, destination configuration, workflow orchestration, or Application Database access.
- It has no child paths.

## Asset Inventory

- `Cargo.toml`: Package metadata and its path dependencies on the log contract and Application Database contract.
- `src/lib.rs`: The `ServerObservability` producer and its stable error type.
- `src/restore.rs`: The Restore completion result and its paired persisted obligation.
- `src/init.rs`: The Init completion result and its paired persisted obligation.
- `src/authentication.rs`: The fixed local authentication-failure System Log
  result, carrying only a fresh record identifier, the event time, the
  response's correlation identifier, and a constant classification and detail.
- `src/authorization.rs`: The fixed authorization-denial System Log result.
- `src/dependency.rs`: The Audit Log destination-unavailability System Log
  result with typed destination-module and operation context.
- `tests/`: Behavior tests for completion construction, field binding, rejection, and redaction.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../../../docs/log-modules/log-module-design.md` and `../../../../docs/spec.md` before changing what an event records.
- MUST give each event family its own module so this crate stays navigable as monitoring grows.
- MUST build a record and its persisted obligation together from the same fields so a post-commit record cannot drift from the committed obligation.
- MUST add or update focused tests with implementation behavior changes as required by `../../../../docs/testing.md`.

- MUST every field must be pre-redacted and bounded before construction; never place a recovery key, backup content, restored identity, credential, or filesystem path in a record.
- MUST keep error variants free of event content so a rendered error cannot disclose what failed validation.
- MUST obtain the record issuer from `ServerLogAuthority`; never widen the log contract's private constructors.
- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
