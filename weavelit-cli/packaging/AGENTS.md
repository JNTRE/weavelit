# Weavelit CLI Packaging Agent Guide

This directory is reserved for assets that package the separately released
Weavelit CLI. The release artifact installs the client without Rust, source
code, or provider credentials and remains independent of the Weavelit Server
package while compatible with its versioned application interface.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Weavelit CLI release-artifact boundaries.
- It does not own Weavelit CLI source, Server packaging, provider credentials, or Server initialization.
- Child paths own platform-specific packaging assets and installation behavior.

## Asset Inventory

- `macos/`: macOS `arm64` Weavelit CLI packaging boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the authoritative [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8) and the Weavelit CLI requirements before changing release-artifact behavior.
- MUST keep packaging assets separate from source and verify installation against a versioned Server interface when release workflows are introduced.
- MUST record release build, installation, verification, and troubleshooting instructions with the packaged workflow.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST package the Weavelit CLI independently from the Server while respecting the versioned interface compatibility policy.
- Agents MUST NOT include Rust, source code, or provider credentials in an installed Weavelit CLI artifact.
- MUST keep platform-specific packaging behavior in its named child boundary.
