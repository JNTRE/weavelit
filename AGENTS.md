# Weavelit Agent Guide

Weavelit is a self-hosted gateway for AI-assisted operational workflows through deliberately supported Service Modules. This repository contains the Server and Weavelit CLI applications, their supporting documentation, and contribution workflow; this guide routes work to the relevant local guidance.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- The repository owns the Weavelit Server, the separately packaged Weavelit CLI, and their canonical product, security, and technical direction.
- Root-level work covers contribution policy, repository orientation, workspace metadata, and routing into implementation and documentation boundaries.
- Do not introduce product-specific AI instruction shims unless a tool requires one; any such shim must defer to this file.

## Asset Inventory

- `CONTRIBUTING.md`: Branch, pull-request, branch-name, and commit message requirements.
- `README.md`: Repository identification and high-level implementation layout.
- `docs/`: Canonical product, security, terminology, and unresolved-decision documentation; follow `docs/AGENTS.md` for changes within it.
- `docs/spec.md`, `docs/glossary.md`, `docs/vision.md`: Canonical specification, terminology, and project vision (when present).
- `project/`: GitHub Project and Repository binding, Issue invariants and templates; follow its local guide.
- `weavelit-cli/`: Source, tests, and macOS release packaging for the separately packaged Weavelit CLI; follow its local guide.
- `server/`: Source, tests, Web UI, and Debian release packaging for the Weavelit Server; follow its local guide.

## Working Rules

- Before editing, read the nearest `AGENTS.md`, then each parent guide upward to this file.
- Follow [Contribution Guidelines](CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under `docs/`, application documentation MUST comply with the [Documentation Standards](docs/documentation-standards.md), which govern document creation, document edits, file naming, and document organization.
- Use the exact canonical names in `docs/glossary.md` when a term is used and format terms as bold links on first substantive use in the text.
- For changes under `server/` or `weavelit-cli/`, read the matching local guide and the canonical documentation it routes to before editing.
- For every implementation behavior change, add or update focused automated tests in the same change and run the applicable validation required by [the Testing and Validation Policy](docs/testing.md). Document any unavoidable manual verification in the owning documentation.
- Update this inventory when repository-level assets, primary folders, or compatibility shims are added, removed, renamed, or moved.
