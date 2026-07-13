# Production Containerfile Agent Guide

This directory owns the future production Containerfile for the Weavelit
Server. It is reserved for the packaged, verified OCI artifact introduced in
Milestone 14 and must remain isolated from development tooling.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the production Containerfile implementation.
- It does not own the production image contract, Server packaging, or the
  development image implementation.
- `Containerfile` must remain aligned with `docs/containers/prod/spec.md`.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and production Containerfile rules.
- `Containerfile`: Placeholder for the Milestone 14 production OCI image.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Read `../../../docs/containers/prod/spec.md` before changing the Containerfile
  and update that specification in the same change when its contract changes.
- Do not replace the placeholder until the verified Server package, image
  provenance, and production deployment contract are defined.
- Validate an implemented image against its verified packaged Server artifact;
  never compile Server source code at container startup.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep the required heading order and keep this guide under 100 lines.
- Use the exact `Containerfile` name and keep it OCI-compatible; do not encode
  Docker-only behavior.
- Exclude Rust, Cargo, source code, test tooling, and build dependencies from
  the production image.
