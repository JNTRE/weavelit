# Weavelit CLI Client Module Crate Agent Guide

This directory is reserved for the compiled-in Weavelit CLI Client Module
crate. It exposes the Weavelit CLI's authenticated `/api/v1/` request
namespace, validates client requests as Operational Requests, and passes them
to the Server's shared authorization policy.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Weavelit CLI Client Module's Server connection-surface behavior.
- It does not own the separately packaged Weavelit CLI application; that belongs in the dedicated client source tree.
- It does not own Server administration functions, Service Module provider behavior, or provider credentials.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Weavelit CLI Client Module crate-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/client-modules/weavelit-cli/` and `../../../../../docs/clients/weavelit-cli/` before changing Weavelit CLI access or request behavior.
- Keep Server-side request authentication and translation here and local CLI behavior in the separately packaged application.
- Add contract and security tests for routes, credentials, request validation, authorization, and sensitive-data exposure as required by `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Mount Weavelit CLI routes beneath `/api/v1/` on the configured Server HTTPS listener and make them unavailable when this Client Module is disabled.
- Derive caller identity from Server-validated credentials, never from claims supplied by the Weavelit CLI.
- Permit operations-only access; do not accept Weavelit CLI credentials for administrative functions or expose provider or automation credentials.
