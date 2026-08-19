# Container Documentation Agent Guide

This directory defines the canonical OCI container contracts for the Weavelit
Server. The MVP production deployment is the native `.deb` package delivered
in Milestone 8; the production image is a post-MVP option delivered in
Milestone 14. This directory keeps the Milestone 1 development image separate
from that later production image so their toolchain, runtime, and deployment
requirements do not leak into each other.

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
- `dev/`: Development container image documentation.
- `prod/`: Production OCI image documentation.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md` and the
  repository-root `AGENTS.md`.
- Before creating or updating a production document, read the
  [Documentation Standards](../documentation-standards.md) and apply its
  authority, document-type, structure, and writing rules.
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
- Preserve the required heading order and keep this guide under 100 lines.
