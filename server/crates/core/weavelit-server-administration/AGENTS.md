# Server Administration Crate Agent Guide

This crate owns the typed, transport-independent gate between an existing
Administration Plane authorization and future Server-owned administration
workflows.

## Purpose and Scope

- This crate owns the closed administration action families, bounded component
  target, exact-session-bound compound admission, current-session MFA step-up
  proof and five-minute policy, live component-enablement check, non-forgeable
  authorized-action result, and reason-free denial boundary.
- It does not own session or TOTP verification, transport routes, Client Module
  translation, Application Database mutations, Audit Log production, or any
  concrete account, Group, grant, component, Operation, or logging workflow.
- It depends only on the administration authority capability, the existing
  authorization proof, the neutral compiled-in component inventory, and
  bounded Application Database contract types.

## Asset Inventory

- `AGENTS.md`: Local ownership, security, and validation rules.
- `Cargo.toml`: Package metadata, narrow contract dependencies, and the JSON
  diagnostic test dependency.
- `src/lib.rs`: Administration actions, requests, results, step-up policy,
  live enablement contract, and direct tests.
- `tests/contract_boundary.rs`: Driver pinning external compile failures at the
  violating source spans.
- `tests/fixtures/forbidden-administration/`: Detached crate that attempts to
  bypass the authorization proof and forge session or step-up capabilities.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root and the Server Authorization Design.
- Keep `AdministrationPlane::authorize` as the single action gate and require
  `AuthorizedAdministrationAdmission` by value. Never accept a separately
  supplied current-session value at this gate.
- Keep every step-up field and constructor private. Only a direct dependency on
  the authority package may bind a validated session or mint a proof after MFA
  verification.
- Keep `MfaPolicy` and `GrantMutation` as the complete step-up-required set;
  ordinary `Account` and `ComponentOperation` actions do not require step-up.
- Read `ComponentEnablement` through the source on every component or Operation
  check after confirming the target exists in `AvailableComponents`. Never
  retain an enablement snapshot on the plane, session, or result.
- Map a missing, mismatched, rolled-back, or expired proof, an unavailable
  enablement read, and a disabled target to the same `AuthorizationDenied`.
- Run this package's tests during development and `make -C server check` before
  handoff.

## Standards and Conventions

- Update this inventory whenever crate assets are added, removed, renamed, or moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep action and component-kind matches exhaustive and without wildcard arms.
- Keep proof and session diagnostics redacted and every rejection payload-free.
- Do not add a route, mutation, Audit producer call, persistence schema, or
  Client Module dependency to this crate.
