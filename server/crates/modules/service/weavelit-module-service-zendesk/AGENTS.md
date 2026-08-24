# Zendesk Service Module Crate Agent Guide

This directory is reserved for the compiled-in Rust Zendesk Service Module
crate. Zendesk is Weavelit's reference integration for incident follow-up
tickets; its supported Operations must remain deliberately named, validated,
authorized, auditable, and implemented through its one documented Service
Connection type.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Zendesk-specific provider integration and supported Operations.
- It does not own shared Service Module guidance, caller authorization, client request translation, or the Weavelit CLI application.
- Future child paths own only narrower Zendesk guidance that differs from this module boundary.

## Asset Inventory

## Working Rules

- MUST follow [Contribution Guidelines](../../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then each parent `AGENTS.md` upward to the repository root.
- MUST read `../../../../../docs/service-modules/zendesk/`, `../../../../../docs/service-modules/`, and the canonical product and security documents before changing Zendesk behavior.
- MUST keep Zendesk provider authentication, request construction, error mapping, retry, rate-limit, and duplicate-protection behavior in this crate.
- MUST add controlled-fake or recorded-fixture tests for Zendesk request construction and failure behavior, following `../../../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep provider credentials in the trusted Server environment and out of source control, client applications, returned errors, and logs.
- Agents MUST NOT contact Zendesk for invalid, unauthorized, duplicated, or otherwise rejected requests.
- MUST preserve Zendesk-specific design in `../../../../../docs/service-modules/zendesk/` rather than duplicating it here.
