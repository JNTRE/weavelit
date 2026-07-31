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
- Before creating or updating a production document, read the [Documentation Standards](../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Keep MFA Module design aligned with `../security-model.md` and record settled commitments in `../spec.md`.
- Use `../glossary.md` for canonical terminology and record unresolved MFA method or enrollment-lifecycle decisions in `../open-questions.md`.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep TOTP-specific design consistent with the established password-confirmation, single-use provisioning, secret-protection, and no-secret-logging requirements in `../security-model.md`.
- Do not add an MFA method or settle an enrollment-lifecycle choice without a recorded product or technical decision.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
