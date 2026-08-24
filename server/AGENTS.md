# Weavelit Server Source Agent Guide

This directory implements the Ubuntu-packaged Weavelit Server: its executable,
compiled-in modules, Web UI source, tests, and Debian packaging. It is the
trusted application boundary that owns the HTTPS API, policy enforcement,
provider integrations, and provider credentials.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns source and release assets for the Weavelit Server package.
- It does not own the separately packaged Weavelit CLI application; that belongs in the dedicated client source tree.
- Component-specific implementation guidance belongs in the matching canonical documentation under `../docs/` until a child source directory gains distinct local workflow rules.

## Asset Inventory

- `Cargo.lock`: Resolved Rust dependencies for reproducible Server workspace builds.
- `Cargo.toml`: Rust workspace manifest for Server package crates.
- `containers/`: Development and production Containerfiles for the Server.
- `crates/`: Rust crate locations for the Server, Application Database contract and backends, and compiled-in modules.
- `Makefile`: Standard local and CI entry point for the Server Web UI, Rust, and browser smoke-test quality gates.
- `packaging/`: Release packaging assets for the Server package; `packaging/deb/` owns Debian-specific files.
- `rust-toolchain.toml`: Pinned Rust toolchain and required quality-gate components.
- `tests/`: Server-focused integration and end-to-end tests.
- `web-ui/`: TypeScript and React source whose production assets are bundled into the Server package.

## Working Rules

- MUST follow [Contribution Guidelines](../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../docs/), application documentation MUST comply with the [Documentation Standards](../docs/documentation-standards.md); use exact canonical terms from [the glossary](../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Before changing a component, agents MUST read its matching guide under `../docs/server/`, `../docs/client-modules/`, `../docs/mfa-modules/`, `../docs/log-modules/`, or `../docs/service-modules/`.
- MUST make minimal, targeted changes and preserve the existing ownership boundaries between Server crates, Web UI, tests, and packaging.
- MUST update the owning documentation and focused tests with each implementation behavior change, as required by `../docs/testing.md`.
- MUST run `make check` for the complete Server Web UI, Rust, and browser smoke-test quality-gate suite.
- MUST add a child `AGENTS.md` only when that path develops distinct commands, validation, security constraints, or documentation routing.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep the Server, Application Database backend, and compiled-in Client, MFA, Log, and Service Modules under `crates/`.
- MUST keep Web UI source under `web-ui/`; do not create a separately released Web UI application.
- MUST keep Server release packaging under `packaging/` and Server-focused integration or end-to-end tests under `tests/`.
- MUST preserve the canonical boundaries in `../docs/`; link to or update those documents instead of duplicating their decisions here.
