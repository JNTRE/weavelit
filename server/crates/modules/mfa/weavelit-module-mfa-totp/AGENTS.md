# TOTP MFA Module Crate Agent Guide

This directory is reserved for the compiled-in Rust TOTP MFA Module crate. It
implements TOTP-specific enrollment, verification, and protected factor-data
handling while relying on the Server for policy, authorization, session
usability, recovery, audit records, and method enablement.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns TOTP-specific MFA Module behavior.
- It does not own passwords, MFA policy, sessions, recovery, audit records, or module availability decisions.
- Future child paths own only narrower TOTP guidance that differs from this module boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and TOTP MFA Module crate-boundary rules.
- `Cargo.toml`: Crate manifest pinning the `totp-rs` and `percent-encoding` dependencies and their minimal features.
- `src/`: TOTP module source, including the approved RFC 6238 profile, its registration, secret handling, and provisioning text.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/mfa-modules/`, `../../../../../docs/server/authentication/`, and the Security Model before changing TOTP behavior.
- Keep TOTP-specific enrollment, verification, and protected factor-data handling in this crate.
- Keep the verification time a caller-supplied parameter; this crate reads no clock, so its tests stay deterministic without sleeping.
- Keep the secret and the provisioning URI in zeroizing types that redact in `Debug` and implement no `Display`, and never delegate the underlying library's hexadecimal secret rendering.
- Add focused security tests for enrollment, verification, disabled-method rejection, and secret absence from errors and logs, following `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Use maintained TOTP libraries; do not implement the TOTP standard from scratch.
- Keep protected factor data out of logs and returned errors.
- Preserve Server responsibility for MFA policy and account sessions rather than duplicating it here.
