# Server Authorization Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** authorization design that makes the final default-deny decision for each requested **[Operation](../../glossary.md#applications-and-interfaces)**. It separates permission and policy evaluation from caller authentication and service-specific behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server authorization design for named **[Operation](../../glossary.md#applications-and-interfaces)** permissions and policy evaluation, including the grant model, effective-grant union, requirement precedence, and structural default-deny.
- This directory owns the Administration Action Gate, closed action families for Administration and Operation requests, and current-session MFA step-up policy.
- It does not own authentication credential validation, MFA factor code verification, or session validation; those belong in the sibling `../authentication/` directory.
- It does not own the content of the authorization-denial System Log record; that belongs in `../observability/authorization-denial-record-design.md`.
- Detailed Group grant-evaluation and additional-permission decisions that
  remain unsettled belong in `../../open-questions.md`.

## Asset Inventory

- `authorization-design.md`: Canonical implementation design for Server authorization.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep authorization design aligned with `../../security-model.md` and record only settled commitments in `../../spec.md`.
- MUST keep credential validation in `../authentication/` and service-specific **[Operation](../../glossary.md#applications-and-interfaces)** behavior in `../../service-modules/`.

- MUST preserve the Server's final, default-deny, per-**[Operation](../../glossary.md#applications-and-interfaces)** authorization boundary; do not grant broad provider-integration access.
