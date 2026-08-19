# Server Modules Agent Guide

This directory groups the compiled-in Rust modules that extend the Weavelit
Server's supported connection surfaces, authentication factors, log
destinations, and external service operations. These modules are released with
the Server package and remain subject to shared Server authorization and policy.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the shared layout for compiled-in Client, MFA, Log, and Service Modules.
- It does not own Server policy, authorization, module availability configuration, or Application Database backends.
- Child category and implementation paths own their module-specific behavior and documentation routing.

## Asset Inventory

- `client/`: Client Module grouping for client-facing Server connection-surface crates.
- `log/`: Log Module grouping for pre-redacted System and Audit Log destination crates.
- `mfa/`: MFA Module grouping for method-specific factor-handling crates.
- `service/`: Service Module grouping for external-service integration crates and Operations.

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the matching guide under `../../../docs/client-modules/`, `../../../docs/mfa-modules/`, `../../../docs/log-modules/`, or `../../../docs/service-modules/` before changing a module.
- MUST keep each module type in its named category and shared Server policy behavior outside this directory.
- MUST add focused contract, integration, and security tests appropriate to the changed module boundary, following `../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep category directories free of Cargo manifests; each Module package belongs in a child directory named `weavelit-module-<module-type>-<implementation>`, or `weavelit-module-<module-type>` for a shared base crate that owns a contract every implementation of that category serves.
- MUST keep all modules compiled into the Server package; do not introduce runtime-installable plugins.
- MUST keep module request translation and provider behavior subject to final Server authorization and policy evaluation.
- MUST preserve module-specific requirements in their canonical `../../../docs/` boundary rather than duplicating them here.
