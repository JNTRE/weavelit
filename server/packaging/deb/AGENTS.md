# Server Debian Packaging Agent Guide

This directory is reserved for the Debian package assets that install the
Weavelit Server on Ubuntu 26.04 LTS `amd64`. The resulting versioned package
includes the Server and Web UI assets without requiring Rust, source code, or
development tooling on the host.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Debian-specific Server package files and installation behavior.
- It does not own Server source, Web UI source, runtime initialization, or Service Connection configuration.
- Future child paths own only narrower Debian packaging guidance that differs from this boundary.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the authoritative [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8) and the relevant Server requirements before changing Debian package behavior.
- MUST keep package installation, service definition, non-secret configuration, persistent-state, and log-location behavior here.
- MUST verify release artifacts in a clean, production-like Ubuntu environment when this packaging workflow is introduced.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST install the Server package's service definition and its required non-secret configuration, persistent-state, and log locations.
- MUST stop the Server with `SIGTERM`, which the Server handles as a request to shut down, and set `TimeoutStopSec=infinity` so an admitted irreversible lifecycle transition and any Application Database close it begins can finish before the supervisor kills the process. Any finite force-kill timeout weakens this no-interruption guarantee.
- Agents MUST NOT create application users, configure Service Connections, complete Init, or start normal operation during package installation.
- MUST keep Debian packaging behavior aligned with the documented Ubuntu 26.04 LTS `amd64` support requirement.
