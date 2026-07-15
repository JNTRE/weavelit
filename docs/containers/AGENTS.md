# Container Documentation Agent Guide

This directory defines the canonical OCI container contracts for the Weavelit
Server. It keeps the Milestone 1 development image separate from the later
production image so their toolchain, runtime, and deployment requirements do
not leak into each other.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns container-image purpose, build, runtime, configuration,
  persistent-state, secret-injection, and validation boundaries.
- It does not own Server application behavior, Debian packaging, or production
  deployment policy outside the container boundary.
- The `dev/` and `prod/` child directories own the respective image contracts.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and container-documentation rules.
- `dev/`: Development container image specification.
- `prod/`: Production OCI image specification.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md` and the
  repository-root `AGENTS.md`.
- Keep the development image and production OCI image as separate artifacts;
  do not make production behavior a development-image mode.
- Preserve OCI-compatible image contracts. Docker may be documented as a local
  client, but do not require Docker-only image or runtime behavior.
- Record unresolved production container decisions in `../open-questions.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
