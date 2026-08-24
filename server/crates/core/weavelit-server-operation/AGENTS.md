# Server Operation Crate Agent Guide

This crate owns what happens after an authorization decision succeeds: selecting
the Service Connection an authorized Operation runs against, and entering
provider execution. It exists so that the ordering of those steps is carried by
Rust's type system rather than by convention.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns the Service Connection selection an `AuthorizedOperation`
  permits, the refusal of a connection the authorized Operation's own Service
  Module does not own, and the single entry into provider execution.
- It owns the spent-exactly-once property: selection consumes the authorization
  proof by value and execution consumes the selection by value, so one
  authorization justifies at most one Operation against at most one Service
  Connection.
- It does not own the authorization decision, grant evaluation, component
  enablement, session validation, credential verification, route contracts,
  transport, or log delivery. A caller supplies an already-decided proof and the
  candidate connections as inbound values.
- It carries no provider credential. A selection names the connection; the
  protected credential stays in the Application Database until the provider asks
  for it, so an authorization result never holds a secret.
- No provider execution path exists yet, so this crate supplies the shape of the
  Service Module boundary rather than a provider.
- It has no child paths other than its compile-fixture directory.

## Asset Inventory

- `Cargo.toml`: Package metadata, the authorization and Application Database
  dependencies, and the JSON dev-dependency used to read Cargo's compiler
  diagnostics.
- `src/lib.rs`: Crate boundary and public surface.
- `src/selection.rs`: `SelectedServiceConnection`, the proof-consuming
  selection, and the selection-consuming execution entry.
- `tests/proof_consumption.rs`: Driver asserting each forbidden fixture fails to
  compile with its pinned rustc diagnostic.
- `tests/fixtures/unconsumed-proof/`: External crate that attempts to borrow a
  proof instead of spending it, to spend a proof or a selection twice, and to
  assemble a selection without an authorization.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root.
- MUST read the Server Authorization Design, Security Model, Technical
  Specification, and Testing and Validation Policy before changing behavior
  here.
- MUST keep selection taking the proof by value and execution taking the selection by
  value. Never add a borrowing overload, a `Clone` or `Copy` implementation, or
  an accessor that yields an owned proof; each would make an authorization
  reusable and is exactly what the compile fixtures forbid.
- MUST keep every field of `SelectedServiceConnection` private and keep the type
  without a public constructor, so a caller holding a connection identifier
  cannot manufacture a selection and bypass the authorization decision.
- MUST keep selection free of authorization logic. It refuses only a connection whose
  Service Module the proof does not name; it must never grant, widen, or
  re-derive a permission.
- MUST keep the protected credential out of this crate. A selection may name a
  connection but must not carry, load, or render its credential.
- When a fixture's required diagnostic carries no rustc error code, agents MUST pin its
  exact message instead. Pinning nothing would let the fixture pass on any
  compilation failure, including a mistake in the fixture source itself.
- MUST run the package tests during development and `make -C server check` before
  handoff.

- MUST update this inventory whenever crate assets are added, removed, renamed, or
  moved.
