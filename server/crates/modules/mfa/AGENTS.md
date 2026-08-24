# MFA Module Crates Agent Guide

This directory is reserved for compiled-in Rust MFA Module crates, beginning
with TOTP. An MFA Module owns method-specific enrollment, verification, and
protected factor-data handling; the Weavelit Server owns MFA policy,
authorization, session usability, recovery, audit records, and enablement.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared layout for method-specific MFA Module crates.
- It does not own MFA policy, authorization, session usability, recovery, audit records, or module enablement.
- Child paths own specific MFA-method behavior and protected factor-data handling.

## Asset Inventory

- `weavelit-module-mfa-totp/`: TOTP MFA Module crate boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../docs/mfa-modules/`, `../../../../docs/server/authentication/`, and the Security Model before changing MFA behavior.
- MUST keep method-specific factor behavior in child modules and Server-wide MFA policy in the Server boundary.
- MUST add focused security tests for every allowed and denied path and for secret absence from returned errors and logs, following `../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST use maintained third-party libraries for MFA methods where appropriate; do not implement security-sensitive standards from scratch.
- MUST keep MFA Modules compiled into the Server package and unavailable as runtime-installable plugins.
- MUST preserve MFA policy and account-session responsibilities in the Server boundary rather than duplicating them here.
