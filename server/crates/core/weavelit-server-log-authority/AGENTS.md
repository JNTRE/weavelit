# Server Log Authority Crate Agent Guide

This crate supplies the capability key that distinguishes Server-owned logging
authority from an ordinary Log Module. It contains no logging behavior.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the `ServerLogAuthority` capability type and nothing else.
- It does not own record construction, dispatch, destinations, or redaction.
- It has no child paths.

## Asset Inventory

- `Cargo.toml`: Package metadata; this crate takes no dependency.
- `src/lib.rs`: The `ServerLogAuthority` capability type.

## Working Rules

- MUST follow [Contribution Guidelines](../../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../../../docs/), application documentation MUST comply with the [Documentation Standards](../../../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- MUST read `../../../../docs/log-modules/log-module-design.md` before changing what this capability permits.
- MUST depend on this crate only from Server-owned crates that must mint logging authority, currently the Server executable, Server Audit, Server Observability, and the log contract itself.
- Agents MUST NOT add this crate to a Log Module dependency graph; that edge is what the log contract's compile fixtures exist to prevent.

- MUST keep this crate free of dependencies, logic, and state so possession stays the only privilege it conveys.
- Agents MUST NOT reexport `ServerLogAuthority` from `weavelit-server-log`; the `server-authority` compile fixture asserts it stays unreachable there.
- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
