# Weavelit Server Agent Guide

This folder documents the implementation-design boundaries of the **[Weavelit Server](../glossary.md#applications-and-interfaces)**. It routes detailed work on the Server's API, access controls, **[Audit Logs](../glossary.md#applications-and-interfaces)**, and observability to focused child directories while keeping product commitments in the canonical top-level documents.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns shared **[Weavelit Server](../glossary.md#applications-and-interfaces)** implementation-design documentation and routing to server-boundary documentation.
- It does not own product commitments, security requirements, or unresolved decisions; those remain in `../core-statements.md`, `../security-model.md`, and `../open-questions.md`.
- The `api/`, `authentication/`, `authorization/`, `automation-identities/`, `audit/`, `database/`, and `observability/` child directories own detailed documentation for their respective Server boundaries.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Weavelit Server.
- `api/`: Documentation for the Server's authenticated HTTPS application interface.
- `authentication/`: Documentation for the Server's human authentication and Automation Identity credential-validation design.
- `authorization/`: Documentation for the Server's permission and policy-evaluation design.
- `automation-identities/`: Documentation for **[Automation Identity](../glossary.md#identities-and-access)** lifecycle, ownership, and accountability design.
- `audit/`: Documentation for the Server's accountability and Audit Log design.
- `database/`: Documentation for Application Database backend boundaries and their implementation design.
- `init-design.md`: Server-owned Init crate, adapter, bootstrap, secret-file, and error-boundary design.
- `observability/`: Documentation for Server System Log design and future operational diagnosis.
- `server-architecture-design.md`: Shared Server workspace, crate-composition, and lifecycle design rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing this directory, read this `AGENTS.md`, then `../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the
  [Documentation Standards](../documentation-standards.md) and apply its
  authority, document-type, lifecycle, structure, and writing rules.
- Keep shared Server design documentation directly in this folder; place boundary-specific detail in its appropriate child directory.
- Update `../core-statements.md` for settled commitments and `../open-questions.md` for unresolved choices instead of treating local design documentation as their replacement.
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
- Keep provider-integration detail in `../service-modules/` and client-connection detail in `../client-modules/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
