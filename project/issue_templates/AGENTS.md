# Issue Templates Agent Guide

This `project/issue_templates/` directory defines the Markdown bodies
that agents copy into temporary files before creating Weavelit GitHub issues
through `gh issue create`. Each file corresponds to one native GitHub issue
type and preserves a reviewable issue structure without relying on GitHub Issue
Forms.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns the Markdown body structure and native-type instructions for agent-created GitHub issues.
- It does not own live issue metadata such as labels, `Priority`, milestones,
  Project status, or issue relationships. GitHub owns those values.
- This guide applies only to the template files in this directory. Use GitHub
  directly to discover or inspect the issues created from them.

## Asset Inventory

- `bug-template.md`: Body template for issues with the native `bug` type.
- `decision-template.md`: Body template for issues with the native `decision` type.
- `epic-template.md`: Body template for issues with the native `epic` type.
- `feature-template.md`: Body template for issues with the native `feature` type.
- `risk-template.md`: Body template for issues with the native `risk` type.
- `task-template.md`: Body template for issues with the native `task` type.

## Working Rules

- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under [`docs/`](../../docs/), application documentation MUST comply with the [Documentation Standards](../../docs/documentation-standards.md); use exact canonical terms from [the glossary](../../docs/glossary.md), formatting them as bold links on first substantive use.

- Before editing, agents MUST read this `AGENTS.md`, then `../AGENTS.md`, and the
  repository-root `AGENTS.md`.
- MUST select the template whose hidden `Native type` value matches the issue being created.
- Before creating any native issue type, agents MUST supply the Conventional Commit-style
  title required for agent-created issues by `CONTRIBUTING.md`; the title is
  separate from the copied template body.
- MUST copy only the sections beginning with `##` into the temporary body file passed to `gh issue create --body-file`.
- MUST replace every bracketed placeholder before creating the issue.
- MUST pass the template's `Native type` value to `gh issue create --type`.
- MUST apply labels, `Priority`, milestones, Project status, and relationships only
  after the active issue lifecycle workflow validates their live GitHub values.

- MUST update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- MUST keep the required heading order and keep this guide under 100 lines.
- MUST name each file `<native-type>-template.md` using the lowercase native issue type shown in its hidden `Native type` instruction.
- MUST keep each template's hidden creation instructions before the copied `##` body sections.
- MUST keep issue titles out of template bodies; the active issue lifecycle workflow
  validates and passes the title separately at creation time.
- Agents MUST NOT place live issue assignments or Project values in a template body; apply them after issue creation through the documented GitHub workflow.
- MUST keep templates accurate, complete, and aligned with the configured Issue types.
- MUST end Markdown files with a single trailing newline.
