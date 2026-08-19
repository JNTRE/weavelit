# Server API Agent Guide

This folder documents the normal authenticated HTTPS application interface of
the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** and
shared API contract conventions. It will define the stable, versioned contract
through which clients invoke supported
**[Operations](../../glossary.md#applications-and-interfaces)** without
duplicating service-specific behavior. The restricted unauthenticated Init
and Restore lifecycle is owned by the
[Server Lifecycle Design](../lifecycle/lifecycle-design.md), with workflow semantics in
the [Server Init Design](../lifecycle/init/init-design.md) and
[Server Restore Design](../lifecycle/restore/restore-design.md). Shared wire conventions used by
those contracts remain coordinated here.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server API contract design, including request, result, stable client-error presentation and redaction, compatibility, pagination, and idempotency behavior.
- It does not own service-specific **[Operation](../../glossary.md#applications-and-interfaces)** semantics; those belong in `../../service-modules/`.
- It does not own pre-operational availability, database selection, or lifecycle
  gating; those belong in `../lifecycle/lifecycle-design.md`. Init recovery-key delivery
  belongs in `../lifecycle/init/init-design.md`, and Restore backup and private recovery-key
  handling belong in `../lifecycle/restore/restore-design.md`.
- API decisions that are not yet settled remain in `../../open-questions.md` until they can be recorded as a commitment.

## Asset Inventory

- `api-contract-design.md`: Canonical version 1 API contract, covering Client
  Module composition and capability declaration, route organization, result and
  error representation, pagination, idempotency, and compatibility.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST add API contract detail only after the relevant wire-format or compatibility decision is settled; keep unresolved choices in `../../open-questions.md`.
- MUST keep service-specific **[Operation](../../glossary.md#applications-and-interfaces)** inputs and effects in `../../service-modules/`, and use `../../glossary.md` for canonical terminology.

- MUST preserve the Server's API-first, versioned interface, including the restricted
  Init and Restore exceptions and normal authenticated-operation commitments in
  `../../spec.md`.
