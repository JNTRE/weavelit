# Server Core Crates Agent Guide

This directory groups the Weavelit Server runtime and its core lifecycle, Init,
Restore, authentication, authorization, and operation crates. These crates own
Server-wide composition, pre-operational application workflows, and
Server-owned credential, access, and post-authorization execution decisions
rather than component-specific storage or module behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns Server runtime, core orchestration, and Server-owned authentication, authorization, and post-authorization operation crate boundaries.
- It does not own Application Database backends or Client, MFA, Log, and Service Module implementations.
- Child paths own executable, lifecycle, Init, Restore, authentication, authorization, and operation implementation guidance.

## Asset Inventory

- `weavelit-server/`: Weavelit Server executable crate.
- `weavelit-server-administration/`: Typed, transport-independent Administration Plane action gate, current-session MFA step-up policy, and live component-enablement decision.
- `weavelit-server-administration-authority/`: Server-owned capability key for binding validated administration sessions and minting verified step-up proofs.
- `weavelit-server-audit/`: Server-owned construction, pre-redaction, and synchronous delivery of Audit Log records.
- `weavelit-server-authentication/`: Local password authentication core, the closed Argon2 profile allowlist, and session and CSRF secret material.
- `weavelit-server-authorization/`: Group-based authorization decision, the additive effective-grant union, and the unforgeable decision proofs.
- `weavelit-server-components/`: Neutral compiled-in component inventory shared by the workflows that check a deployment's declared components against what this build can actually serve.
- `weavelit-server-init/`: Server-owned new-state workflow: the normalized initialization request, its semantic validation, recovery-key preparation and proof of possession, and construction of complete initial application state.
- `weavelit-server-log/`: Typed Log Module contract and compiled-in catalog.
- `weavelit-server-log-authority/`: Server-owned capability key that gates minting of trusted logging authority.
- `weavelit-server-lifecycle/`: Backend-neutral lifecycle domain, validation, and runtime-supplied Application Database catalog contract.
- `weavelit-server-observability/`: Server-owned construction and pre-redaction of System Log records.
- `weavelit-server-operation/`: Post-authorization Service Connection selection and provider execution, structured so an authorization proof is spent at most once.
- `weavelit-server-recovery-key/`: Canonical age recovery-key encoding shared by Init and Restore, and the delivery-nonce proof of possession that confirms a newly generated private key was retained.
- `weavelit-server-restore/`: Server-owned backup envelope, decryption, compatibility, and restored-state validation.

## Working Rules

- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read the nearest child `AGENTS.md`, then this `AGENTS.md`, `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read the matching canonical design under `../../../docs/server/` before changing runtime, lifecycle, Init, Restore, authentication, or authorization behavior.
- MUST keep component-specific persistence and module behavior in their sibling grouping directories.
- MUST add or update focused tests with implementation behavior changes as required by `../../../docs/testing.md`.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep this grouping directory free of a Cargo manifest; each package belongs in a child directory named for its Cargo package.
- MUST keep runtime and pre-operational orchestration separate from component-specific persistence and module implementations.
