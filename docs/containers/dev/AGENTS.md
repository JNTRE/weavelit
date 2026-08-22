# Development Container Documentation Agent Guide

This directory documents the development OCI image that will let contributors
build, run, test, and restart the Weavelit Server without a host Rust install.
It makes the future image contract explicit without defining production runtime
or deployment behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the development container image contract and its
  validation requirements.
- It does not own the Containerfile implementation, Server application
  behavior, or the production OCI image contract.
- This directory contains the canonical development container documentation.

## Asset Inventory

- `development-container-design.md`: Canonical development container image contract.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST update this documentation when the matching development Containerfile or its
  build, mount, configuration, or validation contract changes.
- MUST keep Docker as a supported local client without requiring Docker-only image or
  runtime behavior.
- MUST record unresolved production-container decisions in `../../open-questions.md`.

- MUST keep the development image distinct from the production OCI image; link to the
  production documentation instead of duplicating its deployment requirements.
