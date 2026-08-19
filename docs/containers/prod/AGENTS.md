# Production Container Documentation Agent Guide

This directory documents the future production OCI image for the Weavelit
Server. It protects the boundary between the Milestone 1 development image and
the later packaged, verified production deployment artifact.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the production OCI image contract and its release
  validation requirements.
- It does not own the Containerfile implementation, Server application
  behavior, or the development image contract.
- This directory contains the canonical production container documentation.

## Asset Inventory

- `production-container-design.md`: Canonical production OCI image contract.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST update this documentation when the matching production Containerfile or its
  packaging, runtime, deployment, or release-validation contract changes.
- MUST keep the production image limited to the same versioned, prebuilt Server
  release output used to assemble the `.deb` package; never route a separate
  Server build, development tooling, or source-build behavior into this
  boundary.
- MUST record unresolved production-container decisions in `../../open-questions.md`.

- MUST keep the production image distinct from the development image; link to the
  development documentation instead of duplicating its toolchain requirements.
