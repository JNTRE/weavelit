# Server Authentication Agent Guide

This folder documents the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** authentication design for human users and validation of credentials presented by **[Automation Identities](../../glossary.md#identities-and-access)**. It applies the established **[Local Authentication](../../glossary.md#identities-and-access)** and **[External Authentication](../../glossary.md#identities-and-access)** commitments without defining implementation choices that remain unsettled.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server authentication design for local human accounts, validation of credentials presented by **[Automation Identities](../../glossary.md#identities-and-access)**, and optional **[External Authentication](../../glossary.md#identities-and-access)**.
- It does not own authorization policy evaluation; that belongs in the sibling `../authorization/` directory.
- Authentication lifecycle and multifactor decisions that remain unsettled belong in `../../open-questions.md`.

## Asset Inventory

- `authentication-design.md`: Canonical implementation design for Server authentication.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep authentication design aligned with `../../security-model.md` and record only settled commitments in `../../spec.md`.
- MUST keep authorization evaluation in `../authorization/` and use `../../glossary.md` for canonical terminology.

- MUST preserve the established **[Local Authentication](../../glossary.md#identities-and-access)** default and optional **[External Authentication](../../glossary.md#identities-and-access)** boundary; do not add unsupported authentication methods as commitments.
