# Production Container Documentation Agent Guide

This directory documents the future production OCI image for the Weavelit
Server. It protects the boundary between the Milestone 1 development image and
the later packaged, verified production deployment artifact.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the production OCI image contract and its release
  validation requirements.
- It does not own the Containerfile implementation, Server application
  behavior, or the development image contract.
- `spec.md` is the canonical production container specification.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing and specification-maintenance rules.
- `spec.md`: Canonical production OCI image contract.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Update this specification when the matching production Containerfile or its
  packaging, runtime, deployment, or release-validation contract changes.
- Keep the production image limited to verified packaged Server artifacts;
  never route development tooling or source-build behavior into this boundary.
- Record unresolved production-container decisions in `../../open-questions.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep the required heading order and keep this guide under 100 lines.
- Keep the production image distinct from the development image; link to the
  development specification instead of duplicating its toolchain requirements.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
