# Project Planning Agent Guide

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- Owns repository GitHub Project binding, planning configuration, and issue templates.
- Does not own application implementation, product requirements, or GitHub Actions configuration.

## Asset Inventory

- `issue_templates/`: Native GitHub Issue body templates and their local guidance.
- `project.yaml`: Repository-to-GitHub-Project binding and planning metadata.

## Working Rules

- For changes under [`docs/`](../docs/), application documentation MUST comply with the [Documentation Standards](../docs/documentation-standards.md); use exact canonical terms from [the glossary](../docs/glossary.md), formatting them as bold links on first substantive use.

- MUST read the nearest guide and each parent guide before changing planning assets.
- MUST follow [Contribution Guidelines](../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- MUST treat the configured GitHub Project, its fields, issue types, and planning state as authoritative for project work.
- MUST read the relevant issue template before creating or revising a GitHub issue.
- MUST keep issue bodies compatible with the configured GitHub Project workflow and preserve required dependency links.
- MUST update this inventory when direct assets are added, removed, renamed, or moved.
