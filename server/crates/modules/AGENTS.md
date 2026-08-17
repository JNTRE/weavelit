# Server Modules Agent Guide

This directory groups the compiled-in Rust modules that extend the Weavelit
Server's supported connection surfaces, authentication factors, log
destinations, and external service operations. These modules are released with
the Server package and remain subject to shared Server authorization and policy.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared layout for compiled-in Client, MFA, Log, and Service Modules.
- It does not own Server policy, authorization, module availability configuration, or Application Database backends.
- Child category and implementation paths own their module-specific behavior and documentation routing.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and compiled-in module-boundary rules.
- `client/`: Client Module grouping for client-facing Server connection-surface crates.
- `log/`: Log Module grouping for pre-redacted System and Audit Log destination crates.
- `mfa/`: MFA Module grouping for method-specific factor-handling crates.
- `service/`: Service Module grouping for external-service integration crates and Operations.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the matching guide under `../../../docs/client-modules/`, `../../../docs/mfa-modules/`, `../../../docs/log-modules/`, or `../../../docs/service-modules/` before changing a module.
- Keep each module type in its named category and shared Server policy behavior outside this directory.
- Add focused contract, integration, and security tests appropriate to the changed module boundary, following `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep category directories free of Cargo manifests; each Module package belongs in a child directory named `weavelit-module-<module-type>-<implementation>`, or `weavelit-module-<module-type>` for a shared base crate that owns a contract every implementation of that category serves.
- Keep all modules compiled into the Server package; do not introduce runtime-installable plugins.
- Keep module request translation and provider behavior subject to final Server authorization and policy evaluation.
- Preserve module-specific requirements in their canonical `../../../docs/` boundary rather than duplicating them here.
