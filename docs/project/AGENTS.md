# Project Documentation Agent Guide

This `docs/project/` directory owns Weavelit's minimal repository and GitHub
Project binding profile, native issue templates, deprecated GitHub planning
compatibility index, and the index's removal condition. It does not define
workflow policy or live GitHub configuration.

## Purpose and Scope

Use this section to understand what this directory owns and what it does not own.

- This directory owns the non-secret `project.yaml` safety binding, native
  issue template bodies, deprecated GitHub planning compatibility index, and
  the index's removal condition.
- It does not own canonical product, security, or technical decisions; those
  remain in the top-level documents under `docs/`.
- It does not own workflow policy or live GitHub configuration. GitHub owns
  Issue, Pull Request, milestone, and Project state and configured values.
- `project.yaml` binds automation to the expected repository and Project
  identity as a non-secret safety profile; it is not live GitHub authority.
- `issue_templates/` owns template body shape and native-type instructions, not
  live issue metadata or Project values.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and documentation-boundary rules for
  project binding, issue templates, and the planning compatibility index.
- `project.yaml`: Minimal non-secret expected repository and Project identity
  binding used as an automation safety check.
- `issue_templates/`: Native issue body templates and their local maintenance
  guide.
- `project-standards.md`: Deprecated compatibility index that routes legacy
  links to current GitHub and repository authorities.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the [parent guide](../AGENTS.md)
  and the [repository-root guide](../../AGENTS.md).
- Before creating or updating a production document, read the
  [Documentation Standards](../documentation-standards.md) and apply its
  authority, document-type, lifecycle, structure, and writing rules.
- Before editing a native issue template, read
  [the issue-template guide](issue_templates/AGENTS.md).
- Read GitHub directly before changing Issue, Pull Request, milestone, or
  Project state. Use `project-standards.md` only to route legacy references to
  current authorities.
- Read live values from GitHub rather than adding them to
  `project-standards.md`.
- Change `project-standards.md` only to correct authority routing or its removal
  condition. Do not restore metadata tables or workflow policy.
- Keep the compatibility index deprecated until its stated removal condition
  is satisfied.

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