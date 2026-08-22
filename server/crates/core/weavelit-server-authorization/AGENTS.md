# Server Authorization Crate Agent Guide

This crate owns the Server's Group-based authorization decision for Human Users:
folding Group grants into effective grants, evaluating one request against the
catalogued component enablement, and producing an unforgeable proof only when
every requirement is satisfied.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns the additive union of Group grants, the separation of the
  Server Administration Permission from operational grants, the catalogued
  Client Module, Service Module, and Operation declarations a decision is
  evaluated against, the two authorization decisions and their requirement
  precedence, the private proof types, and the single reason-free denial.
- It does not own account lookup, persistence, session validation, credential
  verification, route contracts, transport, log record construction, log
  delivery, or which grants a Group holds. A caller supplies the Application
  Database's authorization projection and the catalog as inbound values.
- It takes only one workspace path dependency, on
  `weavelit-server-database`, for the projection and the bounded `Name` type,
  so it cannot reach the transport, the listener, a Client Module, a Service
  Module, or the log contract.
- It has no child paths other than its compile-fixture directory.

## Asset Inventory

- `Cargo.toml`: Package metadata, the single Application Database dependency,
  and the JSON dev-dependency used to read Cargo's compiler diagnostics.
- `src/lib.rs`: Crate boundary and public surface.
- `src/catalog.rs`: Plane declarations, the Client Module, Service Module, and
  Operation declarations, and the catalog that indexes them.
- `src/grants.rs`: `OperationalGrants`, `ServerAdministrationPermission`, and
  the additive fold of the authorization projection into `EffectiveHumanGrants`.
- `src/decision.rs`: The two decisions, their requirement precedence, the
  private proof types, and the single denial.
- `tests/proof_construction.rs`: Driver asserting the forbidden fixture fails to
  compile with the pinned rustc codes.
- `tests/fixtures/forbidden-proof/`: External crate that attempts to forge each
  proof type.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Authorization Design, Security Model, Technical
  Specification, and Testing and Validation Policy before changing behavior
  here.
- MUST keep every proof constructor and proof field private to this crate. A proof
  value must be constructible only on the single successful branch of an
  evaluator, and the forbidden fixture must keep failing with its pinned rustc
  code.
- MUST keep the User Plane evaluator free of the Server Administration Permission.
  It must receive `OperationalGrants` only, so an Administrator cannot
  structurally imply an Operation grant.
- MUST keep every match over a grant kind, a plane, or the permission exhaustive with
  no wildcard arm, so a new requirement variant fails to compile until each
  decision handles it.
- MUST keep an uncatalogued component equivalent to a disabled one, and keep
  Operation grants matched by whole name with no wildcard or prefix form.
- MUST keep the denial reason-free: one denial value, no branch-specific detail, and
  no request or account content in any rendering.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
