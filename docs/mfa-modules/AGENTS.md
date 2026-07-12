# MFA Modules Agent Guide

This folder documents compiled-in server-side **[MFA Modules](../glossary.md#applications-and-interfaces)**, beginning with the TOTP method. It separates a method's enrollment, verification, and protected factor-data design from the Weavelit Server's MFA policy and account-session responsibilities.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns method-specific MFA Module design for enrollment, verification, and protected factor-data handling.
- It does not own MFA policy, authorization, session usability, recovery, audit records, or Module enablement; those remain Server responsibilities defined in `../security-model.md`.
- This guide covers this MFA Module documentation boundary; keep general authentication design in `../server/authentication/` and canonical commitments in the top-level documentation.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for MFA Module design.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Keep MFA Module design aligned with `../security-model.md` and record settled commitments in `../core-statements.md`.
- Use `../glossary.md` for canonical terminology and record unresolved MFA method or enrollment-lifecycle decisions in `../open-questions.md`.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep TOTP-specific design consistent with the established password-confirmation, single-use provisioning, secret-protection, and no-secret-logging requirements in `../security-model.md`.
- Do not add an MFA method or settle an enrollment-lifecycle choice without a recorded product or technical decision.
