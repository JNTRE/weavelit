# Server Audit Crate Agent Guide

This crate is Server Audit: the only producer of complete, pre-redacted Audit
Log records and their synchronous delivery through a supplied destination.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns typed Audit event inputs, bounded record construction,
  Attempt references, and synchronous Audit delivery.
- Its bounded R0 recovery ownership is trusted immutable terminal projection
  export/import and fixed supersession-disposition construction.
- It does not own SQLite persistence, runtime drain or scheduling, destination
  configuration or change execution, client routes, account mutations,
  authorization, mutation sequencing, System Logs, client errors, retries, or
  queues.
- It has no child paths other than its compile-fixture directory.

## Asset Inventory

- `Cargo.toml`: Package metadata, narrowly scoped dependencies, and the JSON
  dev-dependency used to read Cargo's compiler diagnostics.
- `src/lib.rs`: Producer, phase preparation, Attempt retention, delivery, and semantic adaptation between Log-owned recovery values and opaque Application Database storage.
- `src/model.rs`: Closed Audit event, principal, outcome, and safe-reference model.
- `tests/producer.rs`: Public producer behavior tests.
- `tests/sqlite.rs`: Real SQLite destination behavior tests.
- `tests/attempt_reference_construction.rs`: Audit Attempt reference retention.
- `tests/fixtures/forbidden-attempt-reference/`: External crate that attempts to
  construct an Attempt reference privately; must keep failing with its pinned rustc diagnostic.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then `../AGENTS.md`, `../../AGENTS.md`,
  `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../../../docs/server/audit/audit-log-design.md`,
  `../../../../docs/log-modules/log-module-design.md`, and
  `../../../../docs/security-model.md` before changing record content.
- MUST add or update focused tests with behavior changes as required by
  `../../../../docs/testing.md`.

- MUST accept only closed typed facts and bounded safe references; never accept raw
  request, response, database, provider, credential, factor, or payload data.
- MUST keep construction errors and Debug output free of event content.
- MUST obtain the record issuer from `ServerLogAuthority`; never widen the log
  contract's private constructors or expose an Attempt-reference constructor.
- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
