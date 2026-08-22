# TOTP MFA Module Crate Agent Guide

This directory is reserved for the compiled-in Rust TOTP MFA Module crate. It
implements TOTP-specific enrollment, verification, and protected factor-data
handling while relying on the Server for policy, authorization, session
usability, recovery, audit records, and method enablement.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns TOTP-specific MFA Module behavior.
- It does not own passwords, MFA policy, sessions, recovery, audit records, or module availability decisions.
- Future child paths own only narrower TOTP guidance that differs from this module boundary.

## Asset Inventory

- `Cargo.toml`: Crate manifest pinning the `totp-rs` and `percent-encoding` dependencies and their minimal features.
- `src/`: TOTP module source, including the approved RFC 6238 profile, its registration, secret handling, and provisioning text.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/mfa-modules/`, `../../../../../docs/server/authentication/`, and the Security Model before changing TOTP behavior.
- MUST keep TOTP-specific enrollment, verification, and protected factor-data handling in this crate.
- MUST keep the verification time a caller-supplied parameter; this crate reads no clock, so its tests stay deterministic without sleeping.
- MUST keep the secret and the provisioning URI in zeroizing types that redact in `Debug` and implement no `Display`, and never delegate the underlying library's hexadecimal secret rendering.
- MUST add focused security tests for enrollment, verification, disabled-method rejection, and secret absence from errors and logs, following `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST use maintained TOTP libraries; do not implement the TOTP standard from scratch.
- MUST keep protected factor data out of logs and returned errors.
- MUST preserve Server responsibility for MFA policy and account sessions rather than duplicating it here.
