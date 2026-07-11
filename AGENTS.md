# Weavelit Agent Guide

Weavelit will be a self-hosted gateway for AI-assisted operational workflows
through deliberately supported Service Modules. This repository will contain
the gateway's implementation, supporting documentation, and contribution
workflow; this guide routes work to the relevant local guidance.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md` (this file).
3. Tool-specific overlays for runtime behavior only.

## 1. Purpose and Scope

- The repository records the product, security, and technical direction for the Weavelit gateway.
- Root-level work covers contribution policy, repository orientation, workspace metadata, and routing into `docs/`.
- This `AGENTS.md` is the canonical source of repository-wide agent guidance; `docs/AGENTS.md` owns documentation-specific workflow.
- Do not introduce product-specific AI instruction shims unless a tool requires one; any such shim must defer to this file.

## 2. Asset Inventory

- `AGENTS.md`: Canonical repository-wide agent routing and standards.
- `.github/copilot-instructions.md`: Compatibility shim that defers to this guide.
- `CONTRIBUTING.md`: Branch, pull-request, branch-name, and Conventional Commit requirements.
- `README.md`: Concise repository identification as an AI agent service gateway.
- `docs/`: Canonical product, security, terminology, and unresolved-decision documentation; follow `docs/AGENTS.md` for changes within it.
- `docs/glossary.md`: Canonical definitions and naming for Weavelit concepts.

## 3. Usage Guidance

- Before editing, read the nearest `AGENTS.md`, then each parent guide upward to this file.
- Make minimal, targeted changes and preserve the existing document structure unless the task requires a broader revision.
- Read `CONTRIBUTING.md` before preparing branches, commits, or pull requests; work from `dev` through a focused topic branch, not directly on `main`.
- Use `<type>/<short-kebab-case-description>` branch names and `<type>(<scope>): <description>` commit messages with a listed required scope.
- For changes under `docs/`, use the local guide and update the canonical document rather than duplicating product commitments elsewhere.

## 4. Standards and Conventions

- Name the repository authority file `AGENTS.md` (uppercase).
- Keep this root `AGENTS.md` as the canonical repository instruction source.
- Maintain product-specific AI instruction files only as thin shims that link or defer to this root file.
- Keep this root `AGENTS.md` at 100 lines or fewer.
- Preserve the terms and stated boundaries in the canonical documentation when adding or revising product claims.
- Use the exact canonical names in `docs/glossary.md` when naming Weavelit concepts.
- On first substantive use in a written section, format a canonical glossary term as a bold link to its glossary category; later uses in that section may be plain text.
- Update this inventory when repository-level assets, primary folders, or compatibility shims are added, removed, renamed, or moved.
