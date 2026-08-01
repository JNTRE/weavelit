# Server Lifecycle Crate Agent Guide

This crate defines the backend-neutral lifecycle domain and runtime-supplied
Application Database backend catalog used by the Weavelit Server before normal
operation.

## Purpose and Scope

- This crate owns lifecycle record and locator domain types, stable identifiers,
  typed connection declarations and values, common input validation, trusted
  backend context, factory dispatch, classifications, and redacted errors.
- It reuses the Application Database contract and deployment identifier.
- It does not own filesystem persistence, serialization, cryptography, concrete
  backend registration, selection orchestration, startup classification logic,
  workflow mutation, Client Modules, or runtime composition.

## Asset Inventory

- `AGENTS.md`: Local lifecycle contract and validation rules.
- `Cargo.toml`: Package metadata and the backend-neutral database dependency.
- `src/`: Lifecycle domain, errors, catalog validation, and factory contract.
- `tests/`: Fake-backend contract, validation-order, and redaction tests.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root.
- Read the Server Lifecycle Design, Server Architecture Design, Application
  Database Design, Security Model, and Testing and Validation Policy.
- Keep persistence and crypto work in the issue that owns lifecycle anchors;
  do not add placeholders for deferred behavior.
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
- Keep the crate free of SQLite, Client Module, filesystem, crypto, and runtime
  dependencies until an owning issue changes that boundary.
- Reject unknown, duplicate, missing, wrongly typed, misclassified, or oversized
  fields before a backend factory is invoked.
- Pass local paths only through trusted Server context, never through declared
  connection fields.
- Keep public errors payload-free and redact identifiers, values, paths, backend
  failures, and factory internals from diagnostic formatting.
