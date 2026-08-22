# Server Administration Authority Crate Agent Guide

This crate supplies the capability key that distinguishes trusted Server
administration composition from ordinary contract consumers.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This crate owns `ServerAdministrationAuthority` and nothing else.
- It does not own authorization, session or MFA verification, action policy,
  persistence, transport, mutation workflows, or Audit Logs.
- Possession is the capability; obtaining it requires an explicit direct
  manifest dependency that can be reviewed independently.

## Asset Inventory

- `Cargo.toml`: Dependency-free package metadata.
- `src/lib.rs`: The Server administration authority capability.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this guide, then each parent `AGENTS.md` through the
  repository root and the Server Authorization Design.
- MUST depend on this crate only from the administration contract and trusted Server
  runtime composition that atomically binds a successful authorization to its
  validated session or has just completed current-session MFA verification.
- Agents MUST NOT add this dependency to a Client Module, transport contract, Service
  Module, MFA Module, Log Module, database backend, or detached consumer.

- MUST keep this crate free of dependencies, behavior, and state so possession stays
  the only privilege it conveys.
- Agents MUST NOT reexport the authority from the administration contract.
- MUST update this inventory whenever crate assets are added, removed, renamed, or moved.
