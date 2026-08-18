# Server Administration Authority Crate Agent Guide

This crate supplies the capability key that distinguishes trusted Server
administration composition from ordinary contract consumers.

## Purpose and Scope

- This crate owns `ServerAdministrationAuthority` and nothing else.
- It does not own authorization, session or MFA verification, action policy,
  persistence, transport, mutation workflows, or Audit Logs.
- Possession is the capability; obtaining it requires an explicit direct
  manifest dependency that can be reviewed independently.

## Asset Inventory

- `AGENTS.md`: Local capability-boundary and dependency rules.
- `Cargo.toml`: Dependency-free package metadata.
- `src/lib.rs`: The Server administration authority capability.

## Usage Guidance

- Before editing, read this guide, then each parent `AGENTS.md` through the
  repository root and the Server Authorization Design.
- Depend on this crate only from the administration contract and trusted Server
  runtime composition that atomically binds a successful authorization to its
  validated session or has just completed current-session MFA verification.
- Never add this dependency to a Client Module, transport contract, Service
  Module, MFA Module, Log Module, database backend, or detached consumer.

## Standards and Conventions

- Keep this crate free of dependencies, behavior, and state so possession stays
  the only privilege it conveys.
- Do not reexport the authority from the administration contract.
- Update this inventory whenever crate assets are added, removed, renamed, or moved.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
