# Weavelit CLI macOS Packaging Agent Guide

This directory is reserved for assets that package the versioned Weavelit CLI
artifact for macOS 26 and later on Apple Silicon (`arm64`). The installed client
must work without Rust, source code, or provider credentials and authenticate to
a compatible installed Weavelit Server through its versioned HTTPS interface.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns macOS `arm64` Weavelit CLI packaging and installation behavior.
- It does not own Weavelit CLI source, Server packaging, provider credentials, or Server configuration and initialization.
- Future child paths own only narrower macOS packaging guidance that differs from this boundary.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the authoritative [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8) and the Weavelit CLI requirements before changing macOS release behavior.
- MUST keep macOS installation behavior here and verify an installed artifact against an installed compatible Server when this workflow is introduced.
- MUST record macOS build, installation, verification, and troubleshooting instructions with the release workflow.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep this artifact limited to macOS 26 and later on Apple Silicon (`arm64`) until support for another platform is recorded.
- Agents MUST NOT require Rust, source code, or provider credentials to install the released CLI artifact.
- MUST verify installed-client authentication and permitted Operation invocation against the versioned Server interface when packaging is introduced.
