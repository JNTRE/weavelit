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

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/mfa-modules/`, `../../../../../docs/server/authentication/`, and the Security Model before changing TOTP behavior.
- Keep TOTP-specific enrollment, verification, and protected factor-data handling in this crate.
- Add focused security tests for enrollment, verification, disabled-method rejection, and secret absence from errors and logs, following `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Use maintained TOTP libraries; do not implement the TOTP standard from scratch.
- Keep protected factor data out of logs, returned errors, and Admin CLI output.
- Preserve Server responsibility for MFA policy and account sessions rather than duplicating it here.