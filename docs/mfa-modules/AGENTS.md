# MFA Modules Agent Guide

This folder documents compiled-in server-side **[MFA Modules](../glossary.md#applications-and-interfaces)**, beginning with the TOTP method. It separates a method's enrollment, verification, and protected factor-data design from the Weavelit Server's MFA policy and account-session responsibilities.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns method-specific MFA Module design for enrollment, verification, and protected factor-data handling.
- It does not own MFA policy, authorization, session usability, recovery, audit records, or Module enablement; those remain Server responsibilities defined by `../spec.md` and the approved profiles in `../security-model.md`.
- This guide covers this MFA Module documentation boundary; keep general authentication design in `../server/authentication/` and canonical commitments in the top-level documentation.

## Asset Inventory

- `totp-module-design.md`: TOTP MFA Module cryptographic profile, secret handling, and provisioning URI construction.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep MFA Module design aligned with `../security-model.md` and record settled commitments in `../spec.md`.
- Use `../glossary.md` for canonical terminology and record unresolved MFA method or enrollment-lifecycle decisions in `../open-questions.md`.

- MUST keep TOTP-specific design consistent with the established password-confirmation, single-use provisioning, secret-protection, and no-secret-logging requirements in `../security-model.md`.
- MUST NOT add an MFA method or settle an enrollment-lifecycle choice without a recorded product or technical decision.
