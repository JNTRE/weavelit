# Issue Templates Agent Guide

This directory provides the Markdown templates used to create Weavelit GitHub
Issues without maintaining a local issue index or planning-metadata record.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- GitHub Issues own issue titles, bodies, outcomes, acceptance criteria, type,
  state, labels, assignees, priority, relationships, milestone assignments, and
  GitHub Project fields.
- This directory owns the Markdown templates used to create issues.
- GitHub is the only navigation and metadata record for individual issues.
- The Markdown templates define the required initial body structure for
  agent-created issues; the created GitHub Issue becomes authoritative.
- The `.github/jntre/issue_templates/` directory owns the template-specific
  creation workflow; read its `AGENTS.md` before changing a template.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and template-maintenance rules.
- `.github/jntre/issue_templates/`: Canonical Markdown body templates for
  agent-created issues; follow its `AGENTS.md` before editing this boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Read `../../../.github/jntre/issue_templates/AGENTS.md` before changing an agent-created issue body template.
- Create an issue by copying the matching template in
  `../../../.github/jntre/issue_templates/` beginning with `##` into a
  temporary body file, replacing every bracketed placeholder, and passing it to
  `gh issue create --body-file`.
- Set the native issue type with `gh issue create --type`; use the active issue
  lifecycle workflow to discover and validate live labels, `Priority`, GitHub
  Milestones, Project status, and applicable relationships before assignment.
- Read GitHub directly to discover, inspect, or update issue state and
  relationships. Do not create or maintain a local issue index or snapshot.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Do not use GitHub Issue Forms; this repository creates issues through the
  Markdown templates in `.github/jntre/issue_templates/`.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
