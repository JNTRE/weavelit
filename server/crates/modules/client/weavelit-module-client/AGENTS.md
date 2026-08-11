# Shared Client Module Crate Agent Guide

This directory is reserved for the compiled-in shared Client Module crate. It
owns the version 1 request schemas, validation, handlers, fixed response
profile, canonical route paths, and capability declaration that every Client
Module shares, so two Client Modules that declare the same function serve one
implementation and cannot diverge.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared Client Module API contract and the declaration value a per-client crate returns.
- It does not own client-specific presentation, embedded assets, or any per-client connection surface; those belong in the named per-client crate.
- It does not own lifecycle, Init, or Restore availability or semantics, Server
    authorization policy, provider credentials, or provider integration behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and shared Client Module crate-boundary rules.
- `Cargo.toml`: Compiled-in shared Client Module package manifest.
- `src/lib.rs`: Canonical route paths, the pre-operational and operational capability declarations and their mounting, pre-operational status request translation, Application Database selection request translation and its same-origin and CSRF trust gate, the shared fixed-profile response helpers, and contract tests.
- `src/restore.rs`: The two-step Restore submission contract: both canonical
  route paths, the ticket header, every header precondition, the recovery-key
  request schema, the payload-free rejection contract, the two typed success
  envelopes, and the Server-core hooks the declaration is composed over.
- `src/init.rs`: The two-request Init submission contract: both canonical
  route paths, the bounded request schema, every header precondition, the
  proof-of-possession shape check, the payload-free rejection contract, the
  two typed success envelopes, and the Server-core hooks the declaration is
  composed over. Not yet declared by a per-client crate or mounted by a
  runtime composition.
- `src/authentication.rs`: The shared login, session-validation, and logout
  route contract: the three canonical route paths, the login request schema,
  every header and cookie precondition, the payload-free rejection contract,
  and the Server-core hooks the declaration is composed over.
- `src/mfa.rs`: The shared second-factor and enrollment route contract: the
  four canonical route paths (code verification, enrollment from a login
  continuation, self-enrollment from a live session, and enrollment
  confirmation), their request schemas, every header, CSRF, and session
  precondition, the one-time provisioning disclosure, and the Server-core
  hooks each declaration is composed over.
- `src/authorization.rs`: The shared operational authorization denial contract:
  the single `AuthorizationRejection` value, the fixed `403` status and
  `authorization_denied` code, and the byte-identical response every denial
  cause renders, deliberately distinct from the `401` authentication contract.
- `src/cookie.rs`: The closed, bounded session and cross-site request forgery
  cookie effect and its fixed rendered attribute text.
- `src/typed_json.rs`: Bounded typed result and error envelopes for every route
  outside the frozen pre-operational allowlist.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/server/api/api-contract-design.md` before changing route organization, result and error representation, or capability declaration.
- Read `../../../../../docs/server/lifecycle/lifecycle-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-status-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-database-selection-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-restore-design.md`, and `../../../../../docs/client-modules/web-ui/pre-operational-init-design.md` before changing a pre-operational contract.
- Keep client-specific behavior in its named per-client crate and add it here only when every Client Module must observe the same behavior.
- Add contract and security tests for accepted requests, stable responses,
  declared capability presence, and denied access as required by
  `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep presence the declaration: a capability exists only once its collaborators
    were supplied, so a Client Module can neither claim a capability it did not
    supply nor supply one it did not claim.
- Mount every declared capability at its canonical route constant; do not
    namespace a route by Client Module and do not let a module compose its own
    listener.
- Emit only compile-time response bodies from the fixed pre-operational profile;
    never emit a cookie, CORS, or diagnostic header from it.
- Derive caller identity from Server-validated credentials or sessions and pass every accepted request to the shared Server authorization policy.
