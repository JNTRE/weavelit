# Service Module Crates Agent Guide

This directory is reserved for compiled-in Rust Service Module crates. Each
Service Module authenticates with one named external service through exactly
one Service Connection type and implements that service's explicitly supported
Operations within the trusted Weavelit Server environment.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the shared layout for service-specific provider integration crates.
- It does not own Client Module request translation, client applications, Server authorization policy, or Service Connection grants.
- Child paths own each named service's Operations, provider authentication, configuration, and failure behavior.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Service Module crate-boundary rules.
- `zendesk/`: Zendesk Service Module crate boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../docs/service-modules/` and the named service's guide before changing provider integration behavior.
- Keep each provider's behavior in its named child crate and use the shared Server authorization result rather than creating local access policy.
- Add controlled-fake or recorded-fixture tests for provider requests, errors, retries, rate limits, and duplicate protection, following `../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Support exactly one Service Connection type per Service Module; represent another connection type as a separately named Service Module.
- Keep provider credentials and provider-specific authentication, retries, and error handling inside the trusted Server environment.
- Do not add a provider capability without documented Operations, permissions, authentication, failure behavior, and maintenance responsibility.