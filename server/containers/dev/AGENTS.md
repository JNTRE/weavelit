# Development Containerfile Agent Guide

This directory owns the development Containerfile for the Weavelit Server.
The implemented image provides an OCI-compatible Rust development environment
without defining the Server's production deployment artifact.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the development Containerfile implementation.
- It does not own the development image contract, Server configuration, or the
  production image implementation.
- `Containerfile` must remain aligned with the canonical documentation in `docs/containers/dev/`.

## Asset Inventory

- `Containerfile`: Implemented Ubuntu 26.04 LTS development image for the Weavelit Server.
- `run-local-server.sh`: Container-local launcher for the host-loopback Web UI testing route.

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- MUST read the canonical documentation in `../../../docs/containers/dev/` before changing
  the Containerfile and update it in the same change when its contract changes.
- MUST validate changes using `make container-check` and preserve its documented
  Docker and OCI-compatible validation.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep the required heading order and keep this guide under 100 lines.
- MUST use the exact `Containerfile` name and keep it OCI-compatible; do not encode
  Docker-only behavior.
- Agents MUST NOT place secrets in build arguments, image layers, image environment
  variables, or the Containerfile.
