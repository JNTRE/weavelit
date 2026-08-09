# Project Planning Agent Guide

This directory is the unified home for Weavelit's delivery-planning
documentation, the GitHub Project binding, and the agent-authored issue
templates.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns repository documentation organized around delivery planning, the project binding file, and issue templates.
- It does not own canonical product, security, or technical decisions; those remain in the top-level documents under `docs/`.
- GitHub Issues own issue titles, bodies, outcomes, acceptance criteria, type, state, labels, assignees, priority, relationships, milestone assignments, and GitHub Project fields.
- GitHub Milestones own milestone titles, summaries, goals, state, dates, progress, and assigned issues.
- The `issue_templates/` directory in this directory owns GitHub Issue templates.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and planning-documentation boundary rules.
- `project-standards.md`: Deprecated compatibility index that routes existing links to current GitHub and repository authorities.
- `project.yaml`: Minimal tracked repository and GitHub Project binding for JNTRE workflow operations; it does not duplicate live GitHub configuration.
- `issue_templates/`: Agent-authored GitHub Issue body templates; follow `issue_templates/AGENTS.md` before editing.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the
  [Documentation Standards](../documentation-standards.md) and apply its
  authority, document-type, lifecycle, structure, and writing rules.
- Read the authoritative GitHub Issue directly before changing an issue.
- Read GitHub directly before changing Issue, Pull Request, milestone, or
  Project state. Use `project-standards.md` only to route legacy references to current authorities.
- Select issue templates from the `issue_templates/` directory in this directory; follow `issue_templates/AGENTS.md` for creation instructions.
- Keep planning outcomes aligned with canonical documents for settled product, security, and technical decisions instead of redefining those decisions.
- Keep GitHub Issues as the only issue navigation and metadata record; do not
  create local issue snapshots.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
