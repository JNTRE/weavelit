# Server Lifecycle Crate Agent Guide

This crate defines the backend-neutral lifecycle domain and runtime-supplied
Application Database backend catalog used by the Weavelit Server before normal
operation.

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

- `AGENTS.md`: Local lifecycle contract and validation rules.
- `Cargo.toml`: Package metadata, exact protected-format and safe-filesystem
  dependencies, and isolated test support.
- `src/`: Lifecycle domain, errors, catalog validation, database selection and
  restart reopening, startup classification, workflow arbitration,
  sealed-deployment state loading, protected formats, trusted-root operations,
  anchor store, and factory contract.
- `tests/`: Fake-backend contract, real-filesystem persistence, selection
  eligibility and replacement, restart reopening, startup classification matrix,
  record advancement, workflow arbitration and contention, crash-point ordering,
  validation-order, malformed/tampered input, and redaction tests.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root.
- Read the Server Lifecycle Design, Server Architecture Design, Application
  Database Design, Security Model, and Testing and Validation Policy.
- Preserve the exact anchor profile and known-answer vector in the Server
  Lifecycle Design; format changes require an explicit migration decision.
- Run the package tests during development and `make -C server check` before
  handoff.

## Standards and Conventions

- Update this inventory whenever crate assets are added, removed, renamed, or
  moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the crate free of SQLite, Client Module, and runtime dependencies; it
  must not open an Application Database during anchor persistence.
- Keep filesystem operations relative to one validated root handle, reject
  unsafe entries, and never weaken synchronization, locking, or no-follow rules.
- Keep raw locator and record persistence gated by crate-owned unforgeable
  permits; expose only authority methods after their owning issue implements
  eligibility checks.
- Keep key and decrypted payload buffers under maintained zeroization and never
  add custom cryptographic primitives.
- Reject unknown, duplicate, missing, wrongly typed, misclassified, or oversized
  fields before a backend factory is invoked.
- Pass local paths only through trusted Server context, never through declared
  connection fields.
- Keep public errors payload-free and redact identifiers, values, paths, backend
  failures, and factory internals from diagnostic formatting.
