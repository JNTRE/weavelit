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
- This directory contains the canonical development container documentation.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing and documentation-maintenance rules.
- `development-container-design.md`: Canonical development container image contract.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Update this documentation when the matching development Containerfile or its
  build, mount, configuration, or validation contract changes.
- Keep Docker as a supported local client without requiring Docker-only image or
  runtime behavior.
- Record unresolved production-container decisions in `../../open-questions.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Keep the required heading order and keep this guide under 100 lines.
- Keep the development image distinct from the production OCI image; link to the
  production documentation instead of duplicating its deployment requirements.
