# Development Containerfile Agent Guide

This directory owns the development Containerfile for the Weavelit Server.
Its future image will provide an OCI-compatible Rust development environment
without defining the Server's production deployment artifact.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the development Containerfile implementation.
- It does not own the development image contract, Server configuration, or the
  production image implementation.
- `Containerfile` must remain aligned with the canonical documentation in `docs/containers/dev/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and development Containerfile rules.
- `Containerfile`: Placeholder for the Milestone 1 development OCI image.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Read the canonical documentation in `../../../docs/containers/dev/` before changing
  the Containerfile and update it in the same change when its contract changes.
- Do not replace the placeholder until the Server development configuration,
  protected state path, deployment-record, database-locator, and secret-reference
  mounts, and restricted pre-operational startup behavior are defined.
- Validate an implemented image using `make check` within the mounted source
  tree and preserve its documented Docker and OCI-compatible validation.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Use the exact `Containerfile` name and keep it OCI-compatible; do not encode
  Docker-only behavior.
- Never place secrets in build arguments, image layers, image environment
  variables, or the Containerfile.
