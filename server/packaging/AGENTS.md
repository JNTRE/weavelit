# Server Packaging Agent Guide

This directory is reserved for assets that package the Weavelit Server for
installation. The Server package includes the Server executable and Web UI
assets; it does not install source code or development tooling on the supported
host.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Server package layout and release-artifact boundaries.
- It does not own Server or Web UI source, application configuration, Init state, or Service Connection setup.
- Child paths own platform-specific release packaging assets.

## Asset Inventory

- `deb/`: Debian package asset boundary for the Server release.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the authoritative [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8) and the relevant Server requirements before changing package behavior.
- MUST keep packaging assets separate from application source and test package behavior in a production-like environment when packaging is introduced.
- MUST record release build, installation, initialization, verification, and troubleshooting instructions with the packaged workflow.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST package the Server and its Web UI assets as one Server release artifact.
- Agents MUST NOT make package installation create application users, configure Service Connections, complete Init, or start normal Server operation against uninitialized state.
- MUST keep platform-specific package files in their named child boundary.
