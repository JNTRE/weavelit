# Web UI Agent Guide

This folder documents the **[Web UI](../../glossary.md#applications-and-interfaces)**, Weavelit's browser-based administrative client. It is the dedicated home for Web UI detail within the broader client-application documentation boundary.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns documentation specific to the Web UI application.
- It does not own shared client-application documentation; that belongs in the parent `../` directory.
- It does not own the server-side Client Module that provides the Web UI connection surface; that belongs in `../../client-modules/`.

## Asset Inventory

- `web-ui-application-design.md`: Build toolchain, generated production outputs, application shell, status presentation states, the first-launch Init and Restore choice, the Application Database selection control, the Init workflow, the Restore submission control, the sign-in control, and the authenticated Accounts workspace for the Web UI browser application.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST keep first-launch Init and Restore presentation and client-side usability
  behavior here; keep their Server connection contract in
  `../../client-modules/web-ui/`, shared lifecycle authority in
  `../../server/lifecycle/lifecycle-design.md`, and workflow authority in
  `../../server/lifecycle/init/init-design.md` and `../../server/lifecycle/restore/restore-design.md`.
- MUST keep Web UI-specific material directly in this folder; move shared client-application material to the parent `../` directory.
- Use `../../glossary.md` for canonical terminology and link to its owning category rather than restating canonical definitions.

- MUST keep server-side connection-surface documentation in `../../client-modules/` and provider-integration documentation in `../../service-modules/`.
