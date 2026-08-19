# Server User Stories Agent Guide

This folder owns the user-visible **[Init](../../glossary.md#states-and-requests)** and **[Restore](../../glossary.md#states-and-requests)** narratives for the **[Web UI](../../glossary.md#applications-and-interfaces)**. It translates Server lifecycle contracts into interaction sequences, user responsibilities, visible transitions, and interrupted-workflow behavior without redefining the implementation designs in the parent directory.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns user stories for Web UI workflows that invoke the Server's restricted pre-operational Init and Restore contracts.
- It does not own lifecycle, persistence, request-processing, or security design; those rules remain in the parent Server design documents and the canonical documentation they reference.
- No child directory currently defines a narrower documentation boundary; add one only when its workflow or policy differs from this guide.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for Server user stories.
- `init-user-story.md`: Web UI first-launch Init sequence, user responsibilities, visible transitions, and interrupted-workflow behavior.
- `restore-user-story.md`: Web UI Restore sequence, user responsibilities, visible transitions, and interrupted-workflow behavior.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, then `../../AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, structure, and writing rules.
- Update the parent Init, Restore, or lifecycle design when Server contract behavior changes; update a user story here for the resulting user-visible sequence and responsibilities.
- Make minimal, targeted edits and preserve each user story's workflow-oriented structure unless the task requires a broader revision.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Keep user-visible workflow narratives in this directory and Server implementation contracts in the parent design documents.
