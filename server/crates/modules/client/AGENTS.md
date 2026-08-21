# Client Module Crates Agent Guide

This directory is reserved for compiled-in Client Module crates that provide
client-facing connection surfaces to the Weavelit Server. A Client Module
authenticates and translates its client's requests into the shared Operation
contract; the Server remains the final authorization authority.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared Client Module crate layout.
- It does not own client-application behavior, Server authorization policy, or Service Module provider integrations.
- `weavelit-module-client/` owns the shared API contract and capability declaration; each per-client path owns only what genuinely differs for its named client.

## Asset Inventory

- `weavelit-module-client/`: Shared Client Module contract crate boundary, including strict account and Group Administration routes.
- `weavelit-module-client-cli/`: Weavelit CLI Client Module crate boundary.
- `weavelit-module-client-webui/`: Web UI Client Module crate boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, `../../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the matching guide under `../../../../docs/client-modules/` before changing a Client Module connection surface.
- MUST read `../../../../docs/clients/` when a change affects the corresponding client application's behavior.
- MUST add contract and security tests for changed accepted requests, stable responses, identity derivation, and denied access as required by `../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep shared contract behavior in `weavelit-module-client/` rather than duplicating it in a per-client crate.
- MUST derive caller identity from Server-validated credentials or sessions; never trust identity, group, or permission claims supplied by a client.
- MUST pass every accepted request to the shared Server authorization policy.
- MUST keep client-application behavior in its named application boundary rather than duplicating it in a Client Module crate.
