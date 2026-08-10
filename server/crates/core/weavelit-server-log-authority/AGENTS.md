# Server Log Authority Crate Agent Guide

This crate supplies the capability key that distinguishes Server-owned logging
authority from an ordinary Log Module. It contains no logging behavior.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the `ServerLogAuthority` capability type and nothing else.
- It does not own record construction, dispatch, destinations, or redaction.
- It has no child paths.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and capability-boundary rules.
- `Cargo.toml`: Package metadata; this crate takes no dependency.
- `src/lib.rs`: The `ServerLogAuthority` capability type.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../../docs/log-modules/log-module-design.md` before changing what this capability permits.
- Depend on this crate only from Server-owned crates that must mint logging authority, currently the Server executable, Server Observability, and the log contract itself.
- Never add this crate to a Log Module dependency graph; that edge is what the log contract's compile fixtures exist to prevent.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Keep this crate free of dependencies, logic, and state so possession stays the only privilege it conveys.
- Do not reexport `ServerLogAuthority` from `weavelit-server-log`; the `server-authority` compile fixture asserts it stays unreachable there.
- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
