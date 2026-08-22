# Weavelit CLI Application Source Agent Guide

This directory is reserved for the Weavelit CLI application source. The CLI
runs on a user's local macOS system, authenticates to the Weavelit Server, and
submits User Plane and Administration Plane requests through the Weavelit CLI
Client Module's versioned HTTPS API. It does not contain provider credentials,
provider integrations, Server authorization policy, or Server administration
behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Weavelit CLI application behavior and local client concerns.
- It does not own the Server-side Weavelit CLI Client Module, Server policy, provider integration, or provider credentials.
- Future child paths own only narrower application guidance that differs from this source boundary.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../docs/clients/weavelit-cli/` for application requirements and `../../docs/client-modules/weavelit-cli/` for the Server connection boundary before changing behavior.
- MUST keep local command workflow and structured result handling here; leave identity derivation, authorization, administration behavior, and provider work with the Server.
- MUST add focused end-to-end or smoke tests for sign-in, sign-out, permitted and denied User Plane and Administration Plane requests, and expected client failure behavior, following `../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST use the configured Server HTTPS listener and `/api/v1/` routes for supported Operations; do not use Web UI browser routes.
- MUST implement commands only for the User Plane and Administration Plane declared by the Weavelit CLI Client Module; command visibility is a usability control rather than authorization.
- Agents MUST NOT add provider credentials, provider-integration logic, or Server authorization policy to the Weavelit CLI.
- MUST preserve Server-owned authorization decisions and canonical API requirements instead of duplicating them locally.
