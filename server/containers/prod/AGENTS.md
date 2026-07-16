# Production Containerfile Agent Guide

This directory owns the future production Containerfile for the Weavelit
Server. It is reserved for the packaged, verified OCI artifact introduced in
Milestone 14 and must remain isolated from development tooling.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the production Containerfile implementation.
- It does not own the production image contract, Server packaging, or the
  development image implementation.
- `Containerfile` must remain aligned with the canonical documentation in `docs/containers/prod/`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and production Containerfile rules.
- `Containerfile`: Placeholder for the Milestone 14 production OCI image.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Read the canonical documentation in `../../../docs/containers/prod/` before changing
  the Containerfile and update it in the same change when its contract changes.
- Do not replace the placeholder until the verified Server package, image
  provenance, and production deployment contract are defined.
- Validate an implemented image against its verified packaged Server artifact;
  never compile Server source code at container startup.

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
- Exclude Rust, Cargo, source code, test tooling, and build dependencies from
  the production image.
