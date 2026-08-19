# Server Audit Crate Agent Guide

This crate is Server Audit: the only producer of complete, pre-redacted Audit
Log records and their synchronous delivery through a supplied destination.

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

- `AGENTS.md`: Local routing, inventory, and Audit producer boundary rules.
- `Cargo.toml`: Package metadata, narrowly scoped dependencies, and the JSON
  dev-dependency used to read Cargo's compiler diagnostics.
- `src/lib.rs`: Producer, phase preparation, Attempt retention, delivery, and semantic adaptation between Log-owned recovery values and opaque Application Database storage.
- `src/model.rs`: Closed Audit event, principal, outcome, and safe-reference model.
- `tests/producer.rs`: Public producer behavior tests.
- `tests/sqlite.rs`: Real SQLite destination behavior tests.
- `tests/attempt_reference_construction.rs`: Audit Attempt reference retention.
- `tests/fixtures/forbidden-attempt-reference/`: External crate that attempts to
  construct an Attempt reference privately; must keep failing with its pinned rustc diagnostic.

## Usage Guidance

- Before editing, read this guide, then `../AGENTS.md`, `../../AGENTS.md`,
  `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../../docs/server/audit/audit-log-design.md`,
  `../../../../docs/log-modules/log-module-design.md`, and
  `../../../../docs/security-model.md` before changing record content.
- Add or update focused tests with behavior changes as required by
  `../../../../docs/testing.md`.

## Standards and Conventions

- Accept only closed typed facts and bounded safe references; never accept raw
  request, response, database, provider, credential, factor, or payload data.
- Keep construction errors and Debug output free of event content.
- Obtain the record issuer from `ServerLogAuthority`; never widen the log
  contract's private constructors or expose an Attempt-reference constructor.
- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
