# Weavelit CLI Source Agent Guide

This directory implements the separately packaged Weavelit CLI for users'
local macOS systems. It is a peer client of the Weavelit Server and implements
commands and workflows for the User Plane and Administration Plane declared by
the Weavelit CLI Client Module. It never owns provider credentials, provider
integrations, Server authorization policy, or Server administration behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Weavelit CLI source, tests, and macOS release packaging.
- It does not own the server-side Weavelit CLI Client Module; that belongs in `../server/crates/modules/client/weavelit-module-client-cli/`.
- It does not own provider integration, provider credentials, Server authorization decisions, or the behavior of Server-owned administration contracts.

## Asset Inventory

- `packaging/`: Release packaging assets for the Weavelit CLI; `packaging/macos/` owns macOS `arm64` packaging files.
- `src/`: Weavelit CLI application source.
- `tests/`: Weavelit CLI-focused tests, including API workflow coverage.

## Working Rules

- MUST follow [Contribution Guidelines](../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../docs/), application documentation MUST comply with the [Documentation Standards](../docs/documentation-standards.md); use exact canonical terms from [the glossary](../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- MUST read `../docs/clients/weavelit-cli/` for application requirements and `../docs/client-modules/weavelit-cli/` for its Server connection boundary before changing behavior.
- MUST make minimal, targeted changes and keep client concerns separate from Server policy and provider-integration code.
- MUST update the owning documentation and focused tests with each implementation behavior change, as required by `../docs/testing.md`.
- MUST add a child `AGENTS.md` only when that path develops distinct commands, validation, security constraints, or documentation routing.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep Weavelit CLI application source under `src/`, its focused tests under `tests/`, and macOS `arm64` release packaging under `packaging/macos/`.
- MUST implement commands and workflows only for planes declared by the Weavelit CLI Client Module, and submit every application function through that module's versioned API surface.
- Agents MUST NOT add provider credentials, provider-integration logic, or Server authorization policy to this application.
- MUST preserve the versioned HTTPS API boundary and canonical requirements in `../docs/`; link to or update those documents instead of duplicating their decisions here.
