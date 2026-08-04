# GitHub Planning Compatibility Agent Guide

This directory preserves a deprecated compatibility path for GitHub planning
authority links while focused workflows and live GitHub discovery replace the
former monolithic standards reference.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns only the deprecated planning compatibility index and its
  removal condition.
- It does not own workflow policy or live GitHub configuration. GitHub owns
  Issue, Pull Request, milestone, and Project state and configured values.
- `.github/jntre/project.yaml` owns the expected repository and Project
  identity only. The `.github/jntre/issue_templates/` directory owns issue
  templates.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and documentation-boundary rules for
  the GitHub planning compatibility index.
- `project-standards.md`: Deprecated compatibility index that routes existing
  links to current GitHub and repository authorities.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Read live values from GitHub rather than adding them to
  `project-standards.md`.
- Change `project-standards.md` only to correct authority routing or its removal
  condition. Do not restore metadata tables or workflow policy.
- Keep the compatibility page deprecated until its stated removal condition is
  satisfied.

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
