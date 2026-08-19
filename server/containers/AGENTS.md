# Server Containers Agent Guide

This directory contains the Server's OCI-compatible Containerfile boundaries.
It keeps the Milestone 1 development-image implementation separate from the
later production image so local tooling cannot become a production dependency.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server Containerfile placement and routes each image to
  its canonical documentation contract.
- It does not own Server source, application configuration, secrets, or Docker
  Compose-style runtime definitions.
- The `dev/` and `prod/` child directories own their respective Containerfiles.

## Asset Inventory

- `dev/`: Development Containerfile boundary; its contract is documented in `docs/containers/dev/`.
- `prod/`: Production Containerfile boundary; its contract is documented in `docs/containers/prod/`.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md` and the
  repository-root `AGENTS.md`.
- MUST read the matching canonical documentation under `../../docs/containers/<image>/`
  before changing a Containerfile, and update it in the same change when the image contract changes.
- MUST keep development and production image implementations separate; do not make
  production behavior a development-image mode.
- MUST preserve OCI-compatible image behavior. Docker may be used locally, but do
  not require Docker-only build or runtime features.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep the required heading order and keep this guide under 100 lines.
- MUST keep container build-context exclusions in `../.dockerignore`, not in a
  Containerfile or runtime configuration.
- Agents MUST NOT add a Compose file until the Server configuration, state, secret-file,
  and startup contracts are documented.
