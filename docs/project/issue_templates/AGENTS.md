# Issue Templates Agent Guide

This `docs/project/issue_templates/` directory defines the Markdown bodies
that agents copy into temporary files before creating Weavelit GitHub issues
through `gh issue create`. Each file corresponds to one native GitHub issue
type and preserves a reviewable issue structure without relying on GitHub Issue
Forms.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Markdown body structure and native-type instructions for agent-created GitHub issues.
- It does not own live issue metadata such as labels, `Priority`, milestones,
  Project status, or issue relationships. GitHub owns those values, and the
  [GitHub Planning Compatibility Index](../project-standards.md)
  routes legacy references to current authorities.
- This guide applies only to the template files in this directory. Use GitHub
  directly to discover or inspect the issues created from them.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and template-maintenance rules.
- `bug-template.md`: Body template for issues with the native `bug` type.
- `decision-template.md`: Body template for issues with the native `decision` type.
- `epic-template.md`: Body template for issues with the native `epic` type.
- `feature-template.md`: Body template for issues with the native `feature` type.
- `risk-template.md`: Body template for issues with the native `risk` type.
- `task-template.md`: Body template for issues with the native `task` type.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Select the template whose hidden `Native type` value matches the issue being created.
- Copy only the sections beginning with `##` into the temporary body file passed to `gh issue create --body-file`.
- Replace every bracketed placeholder before creating the issue.
- Pass the template's `Native type` value to `gh issue create --type`.
- Apply labels, `Priority`, milestones, Project status, and relationships only
  after the active issue lifecycle workflow validates their live GitHub values.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Keep the required heading order and keep this guide under 100 lines.
- Name each file `<native-type>-template.md` using the lowercase native issue type shown in its hidden `Native type` instruction.
- Keep each template's hidden creation instructions before the copied `##` body sections.
- Keep the `## Related Documents` section at the end of each template and link
  it to [GitHub Planning Compatibility Index](../project-standards.md).
- Do not place live issue assignments or Project values in a template body; apply them after issue creation through the documented GitHub workflow.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.