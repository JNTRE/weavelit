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
- `client/`: Client Module crate boundaries for client-facing Server connection surfaces.
- `log/`: Log Module crate boundary for pre-redacted System and Audit Log destinations.
- `mfa/`: MFA Module crate boundaries for method-specific factor handling.
- `service/`: Service Module crate boundaries for external service integrations and Operations.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the matching guide under `../../../docs/client-modules/`, `../../../docs/mfa-modules/`, `../../../docs/log-modules/`, or `../../../docs/service-modules/` before changing a module.
- Keep each module type in its named category and shared Server policy behavior outside this directory.
- Add focused contract, integration, and security tests appropriate to the changed module boundary, following `../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep all modules compiled into the Server package; do not introduce runtime-installable plugins.
- Keep module request translation and provider behavior subject to final Server authorization and policy evaluation.
- Preserve module-specific requirements in their canonical `../../../docs/` boundary rather than duplicating them here.