# Server Containers Agent Guide

This directory contains the Server's OCI-compatible Containerfile boundaries.
It keeps the Milestone 1 development-image implementation separate from the
later production image so local tooling cannot become a production dependency.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Server Containerfile placement and routes each image to
  its canonical documentation contract.
- It does not own Server source, application configuration, secrets, or Docker
  Compose-style runtime definitions.
- The `dev/` and `prod/` child directories own their respective Containerfiles.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Containerfile-boundary rules.
- `dev/`: Development Containerfile boundary; its contract is documented in `docs/containers/dev/`.
- `prod/`: Production Containerfile boundary; its contract is documented in `docs/containers/prod/`.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md` and the
  repository-root `AGENTS.md`.
- Read the matching canonical documentation under `../../docs/containers/<image>/`
  before changing a Containerfile, and update it in the same change when the image contract changes.
- Keep development and production image implementations separate; do not make
  production behavior a development-image mode.
- Preserve OCI-compatible image behavior. Docker may be used locally, but do
  not require Docker-only build or runtime features.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Keep container build-context exclusions in `../.dockerignore`, not in a
  Containerfile or runtime configuration.
- Do not add a Compose file until the Server configuration, state, secret-file,
  and startup contracts are documented.
