# MFA Module Crates Agent Guide

This directory is reserved for compiled-in Rust MFA Module crates, beginning
with TOTP. An MFA Module owns method-specific enrollment, verification, and
protected factor-data handling; the Weavelit Server owns MFA policy,
authorization, session usability, recovery, audit records, and enablement.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared layout for method-specific MFA Module crates.
- It does not own MFA policy, authorization, session usability, recovery, audit records, or module enablement.
- Child paths own specific MFA-method behavior and protected factor-data handling.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and MFA Module crate-boundary rules.
- `totp/`: TOTP MFA Module crate boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../docs/mfa-modules/`, `../../../../docs/server/authentication/`, and the Security Model before changing MFA behavior.
- Keep method-specific factor behavior in child modules and Server-wide MFA policy in the Server boundary.
- Add focused security tests for every allowed and denied path and for secret absence from returned errors and logs, following `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Use maintained third-party libraries for MFA methods where appropriate; do not implement security-sensitive standards from scratch.
- Keep MFA Modules compiled into the Server package and unavailable as runtime-installable plugins.
- Preserve MFA policy and account-session responsibilities in the Server boundary rather than duplicating them here.