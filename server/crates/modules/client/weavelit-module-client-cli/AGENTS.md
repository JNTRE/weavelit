# Weavelit CLI Client Module Crate Agent Guide

This directory is reserved for the compiled-in Weavelit CLI Client Module
crate. It exposes the Weavelit CLI's authenticated User Plane and Administration
Plane `/api/v1/` request namespaces, validates and translates client requests,
and passes them to Server-owned contracts and shared authorization policy.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Weavelit CLI Client Module's Server connection-surface behavior.
- It does not own the separately packaged Weavelit CLI application; that belongs in the dedicated client source tree.
- It does not own Server authorization policy, administration behavior, Service Module provider behavior, or provider credentials.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/client-modules/weavelit-cli/` and `../../../../../docs/clients/weavelit-cli/` before changing Weavelit CLI access or request behavior.
- MUST keep Server-side request authentication and translation here and local CLI behavior in the separately packaged application.
- MUST add contract and security tests for routes, credentials, request validation, authorization, and sensitive-data exposure as required by `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST mount Weavelit CLI routes beneath `/api/v1/` on the configured Server HTTPS listener and make them unavailable when this Client Module is disabled.
- MUST derive caller identity from Server-validated credentials, never from claims supplied by the Weavelit CLI.
- MUST compile and register both the declared User Plane and Administration Plane routes, translate accepted requests into Server-owned contracts, and pass every request to shared Server authorization.
- Agents MUST NOT implement authorization decisions or expose provider or automation credentials.
