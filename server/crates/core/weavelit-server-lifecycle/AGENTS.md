# Server Lifecycle Crate Agent Guide

This crate defines the backend-neutral lifecycle domain and runtime-supplied
Application Database backend catalog used by the Weavelit Server before normal
operation.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns lifecycle record and locator domain types, stable identifiers,
  typed connection declarations and values, common input validation, trusted
  backend context, factory dispatch, classifications, startup classification,
  workflow arbitration, protected anchor formats, trusted-root filesystem
  behavior, Application Database selection and restart reopening, and redacted
  errors.
- It reuses the Application Database contract and deployment identifier.
- It does not own concrete backend registration, workflow-specific metadata
  interpretation, sealing, runtime composition, or Client Modules.

## Asset Inventory

- `Cargo.toml`: Package metadata, exact protected-format and safe-filesystem
  dependencies, and isolated test support.
- `src/`: Lifecycle domain, errors, catalog validation, database selection and
  restart reopening, startup classification, workflow arbitration,
  sealed-deployment state loading and open-database handover, protected formats,
  trusted-root operations, anchor store, and factory contract.
- `tests/`: Fake-backend contract, real-filesystem persistence, selection
  eligibility and replacement, restart reopening, startup classification matrix,
  record advancement, workflow arbitration and contention, Init checkpoint
  release and exact reauthorization, crash-point ordering, validation-order,
  malformed/tampered input, and redaction tests.
- `tests/fixtures/forbidden-lifecycle/`: External compile-fail crate proving a
  released Init checkpoint cannot be forged, completed, or sealed outside this
  crate.
- `tests/fixtures/forbidden-database-authority/`: External compile-fail crate
  proving an ordinary Application Database implementor cannot import database
  authority, construct a selected binding, call its lifecycle-private
  constructor, issue a persistence decoder, or decode persisted text without
  one.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Lifecycle Design, Server Architecture Design, Application
  Database Design, Security Model, and Testing and Validation Policy.
- MUST preserve the exact anchor profile and known-answer vector in the Server
  Lifecycle Design; format changes require an explicit migration decision.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- MUST keep the crate free of SQLite, Client Module, and runtime dependencies; it
  must not open an Application Database during anchor persistence.
- MUST keep filesystem operations relative to one validated root handle, reject
  unsafe entries, and never weaken synchronization, locking, or no-follow rules.
- MUST keep raw locator and record persistence gated by crate-owned unforgeable
  permits; expose only authority methods after their owning issue implements
  eligibility checks.
- MUST keep key and decrypted payload buffers under maintained zeroization and never
  add custom cryptographic primitives.
- MUST reject unknown, duplicate, missing, wrongly typed, misclassified, or oversized
  fields before a backend factory is invoked.
- MUST pass local paths only through trusted Server context, never through declared
  connection fields.
- MUST keep public errors payload-free and redact identifiers, values, paths, backend
  failures, and factory internals from diagnostic formatting.
