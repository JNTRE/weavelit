# Operations CLI Source Agent Guide

This directory implements the separately packaged Operations CLI for users'
local macOS systems. It is a peer client of the Weavelit Server and may request
only permitted Operations through the versioned HTTPS API; it never owns
provider credentials, provider integrations, or Server administration.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Operations CLI source, tests, and macOS release packaging.
- It does not own the server-side Operations CLI Client Module; that belongs in `../server/crates/modules/client/operations-cli/`.
- It does not own provider integration, provider credentials, or administrative functions; those remain Weavelit Server responsibilities.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and source-boundary rules for the Operations CLI.
- `packaging/`: Release packaging assets for the Operations CLI; `macos/` owns macOS `arm64` packaging files.
- `src/`: Operations CLI application source.
- `tests/`: Operations CLI-focused tests, including API workflow coverage.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Read `../docs/clients/operations-cli/` for application requirements and `../docs/client-modules/operations-cli/` for its Server connection boundary before changing behavior.
- Make minimal, targeted changes and keep client concerns separate from Server policy and provider-integration code.
- Update the owning documentation and focused tests with each implementation behavior change, as required by `../docs/testing.md`.
- Add a child `AGENTS.md` only when that path develops distinct commands, validation, security constraints, or documentation routing.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Keep Operations CLI application source under `src/`, its focused tests under `tests/`, and macOS `arm64` release packaging under `packaging/macos/`.
- Do not add provider credentials, provider-integration logic, or administrative functions to this application.
- Preserve the versioned HTTPS API boundary and canonical requirements in `../docs/`; link to or update those documents instead of duplicating their decisions here.