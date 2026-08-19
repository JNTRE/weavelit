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
- This directory contains the canonical production container documentation.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing and documentation-maintenance rules.
- `production-container-design.md`: Canonical production OCI image contract.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Update this documentation when the matching production Containerfile or its
  packaging, runtime, deployment, or release-validation contract changes.
- Keep the production image limited to the same versioned, prebuilt Server
  release output used to assemble the `.deb` package; never route a separate
  Server build, development tooling, or source-build behavior into this
  boundary.
- Record unresolved production-container decisions in `../../open-questions.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Keep the required heading order and keep this guide under 100 lines.
- Keep the production image distinct from the development image; link to the
  development documentation instead of duplicating its toolchain requirements.
