# Service Module Crates Agent Guide

This directory is reserved for compiled-in Rust Service Module crates. Each
Service Module authenticates with one named external service through exactly
one Service Connection type and implements that service's explicitly supported
Operations within the trusted Weavelit Server environment.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared layout for service-specific provider integration crates.
- It does not own Client Module request translation, client applications, Server authorization policy, or Service Connection grants.
- Child paths own each named service's Operations, provider authentication, configuration, and failure behavior.

## Asset Inventory

- `weavelit-module-service-zendesk/`: Zendesk Service Module crate boundary.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../docs/service-modules/` and the named service's guide before changing provider integration behavior.
- MUST keep each provider's behavior in its named child crate and use the shared Server authorization result rather than creating local access policy.
- MUST add controlled-fake or recorded-fixture tests for provider requests, errors, retries, rate limits, and duplicate protection, following `../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST support exactly one Service Connection type per Service Module; represent another connection type as a separately named Service Module.
- MUST keep provider credentials and provider-specific authentication, retries, and error handling inside the trusted Server environment.
- Agents MUST NOT add a provider capability without documented Operations, permissions, authentication, failure behavior, and maintenance responsibility.
