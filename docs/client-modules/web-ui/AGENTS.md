# Web UI Client Module Agent Guide

This folder documents the server-side **[Client Module](../../glossary.md#applications-and-interfaces)** that provides the **[Web UI](../../glossary.md#applications-and-interfaces)** connection surface to the Weavelit Server. It keeps the Server's connection-boundary detail separate from the Web UI application itself.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns documentation specific to the Web UI Client Module.
- It does not own Web UI application behavior; that belongs in `../../clients/web-ui/`.
- Documentation shared by Client Modules belongs in the parent `../` directory.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for the Web UI Client Module.
- `pre-operational-status-design.md`: Versioned status-only pre-operational transport contract and Web UI Client Module boundary for Milestone 1.
- `pre-operational-database-selection-design.md`: Versioned pre-operational Application Database selection transport contract, request schema, same-origin and CSRF preconditions, and rejection contract for the Web UI Client Module.
- `pre-operational-restore-design.md`: Versioned pre-operational two-request Restore submission transport contract, the one-time submission ticket, request schemas, artifact bounds, same-origin and CSRF preconditions, and rejection contract for the Web UI Client Module.
- `pre-operational-init-design.md`: Versioned pre-operational two-request Init submission transport contract, request schema, same-origin and CSRF preconditions, browser-side recovery-key proof derivation, and rejection contract for the Web UI Client Module, which declares this capability for composition by the `weavelit-server` runtime.
- `embedded-asset-delivery-design.md`: Compile-time embedded asset allowlist, MIME types, security headers, body bounds, and path-rejection behavior for the Web UI Client Module.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Keep the Web UI Client Module's Init-capable and Restore-capable transport
  behavior here. `../../server/lifecycle/lifecycle-design.md` owns availability and
  database selection, `../../server/lifecycle/init/init-design.md` owns fresh-state secrets and
  recovery-key delivery, and `../../server/lifecycle/restore/restore-design.md` owns backup and
  private recovery-key handling.
- Keep Web UI Client Module documentation directly in this folder; move shared Client Module material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.
- Make minimal, targeted changes and update this inventory when assets are added, removed, renamed, or moved.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep Web UI application documentation in `../../clients/web-ui/` and provider-integration documentation in `../../service-modules/`.
