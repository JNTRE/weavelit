# Milestone Index Agent Guide

This directory provides repository navigation to Weavelit's authoritative
GitHub Milestones without maintaining a second milestone record.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- GitHub Milestones own milestone titles, summaries, goals, state, dates,
  progress, and assigned issues.
- This directory owns only the local milestone navigation index and its brief
  summaries.
- It does not own milestone outcomes or canonical product, security, or
  technical decisions.
- Canonical documents in `docs/` own settled decisions and definitions.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and maintenance rules for milestone navigation.
- `milestones.md`: Navigation to the authoritative GitHub Milestones with brief summaries.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Create, update, close, and assign work to milestones in GitHub first.
- Refresh `milestones.md` when an authoritative GitHub Milestone is added,
  removed, renamed, or receives a materially changed summary.
- Keep each local summary brief and navigational. Do not copy milestone goals,
  state, dates, progress, or assigned issues into the repository.
- Preserve the non-live `Milestone Example` entry as the first entry under
  `Milestones`, including its non-live disclaimer.
- When a milestone requires a settled product, security, or technical
  decision, update the canonical document or architecture decision record and
  keep the GitHub Milestone aligned with that decision.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Keep `milestones.md` as a navigation index, not a second milestone tracker.
- Give each listed live milestone a level-three title, a separate
  `Open GitHub Milestone <number>` link, and a brief summary.
- Link every listed milestone to its authoritative GitHub Milestone.
- If the index and GitHub differ, treat GitHub as authoritative and refresh the
  index.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
- Use the canonical names from `../../glossary.md` and format their first
  substantive use in a section as bold glossary links.
- Link to canonical documents rather than restating settled product, security,
  or technical decisions in a milestone.
