# Server Administration Crate Agent Guide

This crate owns the typed, transport-independent gate between an existing
Administration Plane authorization and Server-owned administration workflows.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns the closed administration action families, bounded account
  read/create/reset/status payloads and exact-session authorized account proof, bounded component
  targets and desired enablement state, exact-session-bound compound admission,
  current-session MFA step-up proof and five-minute policy, live component-operation
  enablement check, non-forgeable authorized-action result, and reason-free denial
  boundary.
- It does not own session or TOTP verification, transport routes, Client Module
  translation, Application Database mutations, Audit Log production, or any
  concrete account, Group, grant, component, Operation, or logging workflow.
- It depends only on the administration authority capability, the existing
  authorization proof, the neutral compiled-in component inventory, and
  bounded Application Database contract types.

## Asset Inventory

- `Cargo.toml`: Package metadata, narrow contract dependencies, and the JSON
  diagnostic test dependency.
- `src/lib.rs`: Administration actions, account status intent, requests, results, step-up policy,
  component inventory and live enablement contracts, and direct tests.
- `tests/contract_boundary.rs`: Driver pinning external compile failures at the
  violating source spans.
- `tests/fixtures/forbidden-administration/`: Detached crate that attempts to
  bypass the authorization proof and forge session or step-up capabilities.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root and the Server Authorization Design.
- MUST keep `AdministrationPlane::authorize` as the single action gate and require
  `AuthorizedAdministrationAdmission` by value. Never accept a separately
  supplied current-session value at this gate.
- MUST keep every step-up field and constructor private. Only a direct dependency on
  the authority package may bind a validated session or mint a proof after MFA
  verification.
- MUST keep `MfaPolicy` and `GrantMutation` as the complete step-up-required set;
  `Account`, `ComponentOperation`, and `ComponentEnablementChange` actions do not
  require step-up.
- MUST read `ComponentEnablement` through the source on every `ComponentOperation`
  check after confirming the target exists in `AvailableComponents`. A
  `ComponentEnablementChange` MUST instead validate exact inventory membership and
  MUST NOT read current enablement. Never retain an enablement snapshot on the
  plane, session, or result.
- MUST map a missing, mismatched, rolled-back, or expired proof, an unavailable
  enablement read, and a disabled target to the same `AuthorizationDenied`.
- MUST run this package's tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or moved.
- MUST keep action and component-kind matches exhaustive and without wildcard arms.
- MUST keep proof and session diagnostics redacted and every rejection payload-free.
- Agents MUST NOT add a route, mutation, Audit producer call, persistence schema, or
  Client Module dependency to this crate.
