# Web UI Client Module Agent Guide

This folder documents the server-side **[Client Module](../../glossary.md#applications-and-interfaces)** that provides the **[Web UI](../../glossary.md#applications-and-interfaces)** connection surface to the Weavelit Server. It keeps the Server's connection-boundary detail separate from the Web UI application itself.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns documentation specific to the Web UI Client Module.
- It does not own Web UI application behavior; that belongs in `../../clients/web-ui/`.
- Documentation shared by Client Modules belongs in the parent `../` directory.

## Asset Inventory

- `pre-operational-status-design.md`: Versioned status-only pre-operational transport contract and Web UI Client Module boundary for Milestone 1.
- `pre-operational-database-selection-design.md`: Versioned pre-operational Application Database selection transport contract, request schema, same-origin and CSRF preconditions, and rejection contract for the Web UI Client Module.
- `pre-operational-restore-design.md`: Versioned pre-operational two-request Restore submission transport contract, the one-time submission ticket, request schemas, artifact bounds, same-origin and CSRF preconditions, and rejection contract for the Web UI Client Module.
- `pre-operational-init-design.md`: Versioned pre-operational two-request Init submission transport contract, request schema, same-origin and CSRF preconditions, browser-side recovery-key proof derivation, and rejection contract for the Web UI Client Module, which declares this capability for composition by the `weavelit-server` runtime.
- `embedded-asset-delivery-design.md`: Compile-time embedded asset allowlist, MIME types, security headers, body bounds, and path-rejection behavior for the Web UI Client Module.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep the Web UI Client Module's Init-capable and Restore-capable transport
  behavior here. `../../server/lifecycle/lifecycle-design.md` owns availability and
  database selection, `../../server/lifecycle/init/init-design.md` owns fresh-state secrets and
  recovery-key delivery, and `../../server/lifecycle/restore/restore-design.md` owns backup and
  private recovery-key handling.
- MUST keep Web UI Client Module documentation directly in this folder; move shared Client Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep Web UI application documentation in `../../clients/web-ui/` and provider-integration documentation in `../../service-modules/`.
