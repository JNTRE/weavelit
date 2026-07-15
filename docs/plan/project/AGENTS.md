# GitHub Planning Standards Agent Guide

This directory records the active GitHub issue metadata and Weavelit GitHub
Project workflow values used to plan delivery. It provides a stable local
reference while GitHub remains the source of truth for the live configuration.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the documented standards for GitHub issue types, labels, organization-level Issue Fields, and Project status values.
- It does not own the live GitHub configuration; refresh the reference from the [Weavelit GitHub Project](https://github.com/orgs/JNTRE/projects/1/views/1) and the `JNTRE/weavelit` issue tracker.
- The sibling `../issues/` directory owns open-issue snapshots, including the metadata values assigned to individual issues.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and documentation-boundary rules for GitHub planning standards.
- `issue-standards.md`: Active GitHub issue-type, label, priority, and Project-status reference.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Refresh type, label, Issue Field, and Project status values from GitHub before changing `issue-standards.md`.
- Preserve the exact configured name and capitalization of each GitHub value in `issue-standards.md`.
- Update `../issues/issues.md` in the same change when a documented metadata change affects its standard record format or displayed values.
- Record proposed GitHub configuration changes only after they are active in GitHub; do not present a proposal as an active standard.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
