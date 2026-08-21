# Shared Client Module Crate Agent Guide

This directory is reserved for the compiled-in shared Client Module crate. It
owns the version 1 request schemas, validation, handlers, fixed response
profile, canonical route paths, and capability declaration that every Client
Module shares, so two Client Modules that declare the same function serve one
implementation and cannot diverge.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared Client Module API contract and the declaration value a per-client crate returns.
- It does not own client-specific presentation, embedded assets, or any per-client connection surface; those belong in the named per-client crate.
- It does not own lifecycle, Init, or Restore availability or semantics, Server
    authorization policy, provider credentials, or provider integration behavior.

## Asset Inventory

- `Cargo.toml`: Compiled-in shared Client Module package manifest.
- `src/lib.rs`: Canonical route paths, the pre-operational and operational capability declarations and their mounting, pre-operational status request translation, Application Database selection request translation and its same-origin and CSRF trust gate, the shared fixed-profile response helpers, the shared release-time clearing owner every collected secret-bearing request body is parsed through, and contract tests.
- `src/administration.rs`: The shared account-list, account-view, and account-status Administration Plane contract: canonical routes, strict session-bearing requests, route-scoped cursor paging, safe public projections reusable by Group member reads, bounded typed envelopes, stable rejections, and Server-core hooks.
- `src/groups.rs`: The shared Group CRUD, membership, direct-grant, and compiled-catalog Administration Plane contract: canonical routes, strict public identifiers and structured grants, route-scoped paging, bounded typed envelopes, stable rejections, and Server-core hooks.
- `src/credential_issuance.rs`: The shared credential-issuance step-up, account-create, and password-reset contracts: canonical strict routes, clearing secret request ownership, single-use ticket transport, one-time temporary-password responses, stable rejections, and Server-core hooks.
- `src/restore.rs`: The two-step Restore submission contract: both canonical
  route paths, the ticket header, every header precondition, the recovery-key
  request schema and the shared release-time clearing of its collected body,
  the payload-free rejection contract, the two typed success envelopes, and the
  Server-core hooks the declaration is composed over.
- `src/init.rs`: The two-request Init submission contract: both canonical
  route paths, the bounded request schema, every header precondition, the
  proof-of-possession shape check, the shared release-time clearing of each
  collected body, the payload-free rejection contract, the two typed success
  envelopes, and the Server-core hooks the declaration is composed over. Not
  yet declared by a per-client crate or mounted by a runtime composition.
- `src/authentication.rs`: The shared login, session-validation, and logout
  route contract: the three canonical route paths, the login request schema and
  the shared release-time clearing of its collected body, every header and
  cookie precondition, the payload-free rejection contract, and the Server-core
  hooks the declaration is composed over.
- `src/mfa.rs`: The shared second-factor and enrollment route contract: the
  four canonical route paths (code verification, enrollment from a login
  continuation, self-enrollment from a live session, and enrollment
  confirmation), their request schemas and the shared release-time clearing of
  each collected body, every header, CSRF, and session precondition, the
  one-time provisioning disclosure, and the Server-core hooks each declaration
  is composed over.
- `src/mfa_policy.rs`: The shared TOTP step-up, account MFA-requirement, and
  enrollment-reset contracts, including strict secret-bearing requests,
  reusable opaque ticket transport, safe account results, and stable rejections.
- `src/password_change.rs`: The restricted-session password-change route contract: its canonical path, strict one-field request, release-time body clearing, session and same-origin preconditions, fresh-session cookie result, stable rejections, and Server-core hook.
- `src/authorization.rs`: The shared operational authorization denial contract:
  the single `AuthorizationRejection` value, the fixed `403` status and
  `authorization_denied` code, and the byte-identical response every denial
  cause renders, deliberately distinct from the `401` authentication contract.
- `src/cookie.rs`: The closed, bounded session and cross-site request forgery
  cookie effect and its fixed rendered attribute text.
- `src/typed_json.rs`: Bounded typed result and error envelopes for every route
  outside the frozen pre-operational allowlist.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/server/api/api-contract-design.md` before changing route organization, result and error representation, or capability declaration.
- MUST read `../../../../../docs/server/lifecycle/lifecycle-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-status-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-database-selection-design.md`, `../../../../../docs/client-modules/web-ui/pre-operational-restore-design.md`, and `../../../../../docs/client-modules/web-ui/pre-operational-init-design.md` before changing a pre-operational contract.
- MUST keep client-specific behavior in its named per-client crate and add it here only when every Client Module must observe the same behavior.
- MUST add contract and security tests for accepted requests, stable responses,
  declared capability presence, and denied access as required by
  `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep presence the declaration: a capability exists only once its collaborators
    were supplied, so a Client Module can neither claim a capability it did not
    supply nor supply one it did not claim.
- MUST mount every declared capability at its canonical route constant; do not
    namespace a route by Client Module and do not let a module compose its own
    listener.
- MUST emit only compile-time response bodies from the fixed pre-operational profile;
    never emit a cookie, CORS, or diagnostic header from it.
- MUST take sole ownership of a request buffer that carries plaintext secret material
    and clear it through a release-time owner, never through a manual call at
    one exit point, so an added early return cannot leave it uncleared.
- MUST derive caller identity from Server-validated credentials or sessions and pass every accepted request to the shared Server authorization policy.
