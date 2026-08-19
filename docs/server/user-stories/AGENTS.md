# Server User Stories Agent Guide

This folder owns the user-visible **[Init](../../glossary.md#states-and-requests)** and **[Restore](../../glossary.md#states-and-requests)** narratives for the **[Web UI](../../glossary.md#applications-and-interfaces)**. It translates Server lifecycle contracts into interaction sequences, user responsibilities, visible transitions, and interrupted-workflow behavior without redefining the implementation designs in the parent directory.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns user stories for Web UI workflows that invoke the Server's restricted pre-operational Init and Restore contracts.
- It does not own lifecycle, persistence, request-processing, or security design; those rules remain in the parent Server design documents and the canonical documentation they reference.
- No child directory currently defines a narrower documentation boundary; add one only when its workflow or policy differs from this guide.

## Asset Inventory

- `init-user-story.md`: Web UI first-launch Init sequence, user responsibilities, visible transitions, and interrupted-workflow behavior.
- `restore-user-story.md`: Web UI Restore sequence, user responsibilities, visible transitions, and interrupted-workflow behavior.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- MUST update the parent Init, Restore, or lifecycle design when Server contract behavior changes; update a user story here for the resulting user-visible sequence and responsibilities.
- MUST make minimal, targeted edits and preserve each user story's workflow-oriented structure unless the task requires a broader revision.

- MUST keep user-visible workflow narratives in this directory and Server implementation contracts in the parent design documents.
