# Zendesk Service Module Crate Agent Guide

This directory is reserved for the compiled-in Rust Zendesk Service Module
crate. Zendesk is Weavelit's reference integration for incident follow-up
tickets; its supported Operations must remain deliberately named, validated,
authorized, auditable, and implemented through its one documented Service
Connection type.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Zendesk-specific provider integration and supported Operations.
- It does not own shared Service Module guidance, caller authorization, client request translation, or the Weavelit CLI application.
- Future child paths own only narrower Zendesk guidance that differs from this module boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Zendesk Service Module crate-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- Read `../../../../../docs/service-modules/zendesk/`, `../../../../../docs/service-modules/`, and the canonical product and security documents before changing Zendesk behavior.
- Keep Zendesk provider authentication, request construction, error mapping, retry, rate-limit, and duplicate-protection behavior in this crate.
- Add controlled-fake or recorded-fixture tests for Zendesk request construction and failure behavior, following `../../../../../docs/testing.md`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep provider credentials in the trusted Server environment and out of source control, client applications, returned errors, and logs.
- Do not contact Zendesk for invalid, unauthorized, duplicated, or otherwise rejected requests.
- Preserve Zendesk-specific design in `../../../../../docs/service-modules/zendesk/` rather than duplicating it here.
