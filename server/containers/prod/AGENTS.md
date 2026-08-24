# Production Containerfile Agent Guide

This directory owns the future production Containerfile for the Weavelit
Server. It is reserved for the OCI wrapper around the versioned, prebuilt Server
release output introduced in Milestone 14 and must remain isolated from
development tooling.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the production Containerfile implementation.
- It does not own the production image contract, Server packaging, or the
  development image implementation.
- `Containerfile` must remain aligned with the canonical documentation in `docs/containers/prod/`.

## Asset Inventory

- `Containerfile`: Placeholder for the Milestone 14 production OCI image.

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- MUST read the canonical documentation in `../../../docs/containers/prod/` before changing
  the Containerfile and update it in the same change when its contract changes.
- Agents MUST NOT replace the placeholder until the versioned, prebuilt Server release
  output used to assemble the `.deb` package, image provenance, and production
  deployment contract are defined.
- MUST validate an implemented image against that same Server release output; do not
  install the `.deb` at any image-build or runtime stage, and do not compile
  Server source code at container startup.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep the required heading order and keep this guide under 100 lines.
- MUST use the exact `Containerfile` name and keep it OCI-compatible; do not encode
  Docker-only behavior.
- MUST exclude Rust, Cargo, source code, test tooling, and build dependencies from
  the production image.
