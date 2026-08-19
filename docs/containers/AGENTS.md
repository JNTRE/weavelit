# Container Documentation Agent Guide

This directory defines the canonical OCI container contracts for the Weavelit
Server. The MVP production deployment is the native `.deb` package delivered
in Milestone 8; the production image is a post-MVP option delivered in
Milestone 14. This directory keeps the Milestone 1 development image separate
from that later production image so their toolchain, runtime, and deployment
requirements do not leak into each other.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns container-image purpose, build, runtime, configuration,
  persistent-state, secret-injection, and validation boundaries.
- It does not own Server application behavior, Debian packaging, or production
  deployment policy outside the container boundary.
- The `dev/` and `prod/` child directories own the respective image contracts.

## Asset Inventory

- `dev/`: Development container image documentation.
- `prod/`: Production OCI image documentation.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST keep the development image and production OCI image as separate artifacts;
  do not make production behavior a development-image mode.
- MUST preserve OCI-compatible image contracts. Docker may be documented as a local
  client, but do not require Docker-only image or runtime behavior.
- MUST record unresolved production container decisions in `../open-questions.md`.
