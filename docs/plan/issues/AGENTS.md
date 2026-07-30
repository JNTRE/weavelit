# Issue Index Agent Guide

This directory provides repository navigation to open Weavelit GitHub Issues
without maintaining a second issue or planning-metadata record.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- GitHub Issues own issue titles, bodies, outcomes, acceptance criteria, type,
  state, labels, assignees, priority, relationships, milestone assignments, and
  GitHub Project fields.
- This directory owns the local open-issue navigation index and the Markdown
  templates used to create issues.
- `issues.md` lists open issues with linked titles, brief summaries, and
  optional Related Epic and Milestone context only.
- The Markdown templates define the required initial body structure for
  agent-created issues; the created GitHub Issue becomes authoritative.
- The `templates/` child directory owns the template-specific creation workflow; read `templates/AGENTS.md` before changing a template.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and issue-index maintenance rules.
- `issues.md`: Navigation to authoritative open GitHub Issues with brief summaries.
- `templates/`: Markdown body templates for agent-created issues; follow `templates/AGENTS.md` before editing this boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Read `templates/AGENTS.md` before changing an agent-created issue body template.
- Create an issue by copying the matching template in `templates/` beginning with `##` into a temporary body file, replacing every bracketed placeholder, and passing it to `gh issue create --body-file`.
- Set the native issue type with `gh issue create --type`; then assign the
  component label, `Priority`, GitHub Milestone, Project status, and applicable
  issue relationships defined in `../project/project-standards.md`.
- Refresh `issues.md` from the repository issue tracker. Add newly opened
  issues, remove closed issues, and update changed titles, summaries, Related
  Epic values, and Milestone values.
- Include each open repository issue exactly once and summarize its stated
  outcome or decision without copying its body or acceptance criteria.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Keep the required heading order and keep this guide under 100 lines.
- Give each listed issue a linked level-three heading in the form
  `#<number> <title>` and a bold `Summary:` paragraph.
- When applicable, use a `Group | Field | Value` table containing only
  `Related | Related epic | <value>` and `Related | Milestone | <value>` rows;
  omit any row whose value is absent.
- Do not copy type, state, labels, assignees, priority, other relationships, or
  GitHub Project fields into `issues.md`.
- Do not use GitHub Issue Forms; this repository creates issues through the Markdown templates in `templates/`.
- Order an epic before its child issues, then order siblings by issue number.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
