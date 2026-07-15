# Development Container Documentation Agent Guide

This directory documents the development OCI image that will let contributors
build, run, test, and restart the Weavelit Server without a host Rust install.
It makes the future image contract explicit without defining production runtime
or deployment behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the development container image contract and its
  validation requirements.
- It does not own the Containerfile implementation, Server application
  behavior, or the production OCI image contract.
- `spec.md` is the canonical development container specification.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing and specification-maintenance rules.
- `spec.md`: Canonical development container image contract.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Update this specification when the matching development Containerfile or its
  build, mount, configuration, or validation contract changes.
- Keep Docker as a supported local client without requiring Docker-only image or
  runtime behavior.
- Record unresolved production-container decisions in `../../open-questions.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep the required heading order and keep this guide under 100 lines.
- Keep the development image distinct from the production OCI image; link to the
  production specification instead of duplicating its deployment requirements.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
