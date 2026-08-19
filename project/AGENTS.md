# Project Planning Agent Guide

This directory is the unified home for Weavelit's GitHub Project binding,
planning invariants, and agent-authored Issue templates.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns repository planning metadata, the project binding file, and Issue templates.
- It does not own canonical product, security, or technical decisions; those remain in the top-level documents under `docs/`.
- GitHub Issues own issue titles, bodies, outcomes, acceptance criteria, type, state, labels, assignees, priority, relationships, milestone assignments, and GitHub Project fields.
- GitHub Milestones own milestone titles, summaries, goals, state, dates, progress, and assigned issues.
- The `issue_templates/` directory in this directory owns GitHub Issue templates.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and planning-documentation boundary rules.
- `project.yaml`: Minimal tracked repository and GitHub Project binding for JNTRE workflow operations; it does not duplicate live GitHub configuration.
- `issue_templates/`: Agent-authored GitHub Issue body templates; follow `issue_templates/AGENTS.md` before editing.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then the repository-root `AGENTS.md`.
- Read the authoritative GitHub Issue directly before changing an issue.
- Read GitHub directly before changing Issue, Pull Request, milestone, or
  Project state.
- Select issue templates from the `issue_templates/` directory in this directory; follow `issue_templates/AGENTS.md` for creation instructions.
- Keep planning outcomes aligned with canonical documents for settled product, security, and technical decisions instead of redefining those decisions.
- Keep GitHub Issues as the only issue navigation and metadata record; do not
  create local issue snapshots.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Keep the required heading order and keep this guide under 100 lines.
- Keep the profile, planning invariants, and templates accurate, complete, and
  located in their owning planning boundary.
- Keep references repository-relative and update them when planning assets move,
  are renamed, or are retired.
