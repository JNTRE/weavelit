# Weavelit Agent Guide

Weavelit is a self-hosted gateway for AI-assisted operational workflows through
deliberately supported Service Modules. This repository contains the Server and
Operations CLI applications, their supporting documentation, and contribution
workflow; this guide routes work to the relevant local guidance.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md` (this file).
3. Tool-specific overlays for runtime behavior only.

## 1. Purpose and Scope

- The repository owns the Weavelit Server, the separately packaged Operations CLI, and their canonical product, security, and technical direction.
- Root-level work covers contribution policy, repository orientation, workspace metadata, and routing into implementation and documentation boundaries.
- This `AGENTS.md` is the canonical source of repository-wide agent guidance; `docs/AGENTS.md` owns documentation-specific workflow.
- Do not introduce product-specific AI instruction shims unless a tool requires one; any such shim must defer to this file.

## 2. Asset Inventory

- `AGENTS.md`: Canonical repository-wide agent routing and standards.
- `.github/`: GitHub-specific repository metadata, including the Copilot
  compatibility shim.
- `.github/copilot-instructions.md`: Compatibility shim that defers to this guide.
- `.gitignore`: Git ignore rules for local macOS, build, backup, and workspace
  artifacts.
- `.vscode/`: Shared VS Code workspace settings.
- `CONTRIBUTING.md`: Branch, pull-request, branch-name, and Conventional Commit requirements.
- `README.md`: Repository identification and high-level implementation layout.
- `docs/`: Canonical product, security, terminology, and unresolved-decision documentation; follow `docs/AGENTS.md` for changes within it.
- `docs/glossary.md`: Canonical definitions and naming for Weavelit concepts.
- `operations-cli/`: Source, tests, and macOS release packaging for the separately packaged Operations CLI; follow its local guide.
- `server/`: Source, tests, Web UI, and Debian release packaging for the Weavelit Server; follow its local guide.
- `weavelit.code-workspace`: VS Code multi-root workspace definition.

## 3. Usage Guidance

- Before editing, read the nearest `AGENTS.md`, then each parent guide upward to this file.
- Make minimal, targeted changes and preserve the existing document structure unless the task requires a broader revision.
- Read `CONTRIBUTING.md` before preparing branches, commits, or pull requests; work from `dev` through a focused topic branch, not directly on `main`.
- Use `<type>/<short-kebab-case-description>` branch names and `<type>(<scope>): <description>` commit messages with a listed required scope.
- For changes under `docs/`, use the local guide and update the canonical document rather than duplicating product commitments elsewhere.
- For changes under `server/` or `operations-cli/`, read the matching local guide and the canonical documentation it routes to before editing.
- For every implementation behavior change, add or update focused automated tests in the same change and run the applicable validation required by [the Testing and Validation Policy](docs/testing.md). Document any unavoidable manual verification in the owning specification.

## 4. Standards and Conventions

- Name the repository authority file `AGENTS.md` (uppercase).
- Keep this root `AGENTS.md` as the canonical repository instruction source.
- Maintain product-specific AI instruction files only as thin shims that link or defer to this root file.
- Keep this root `AGENTS.md` at 100 lines or fewer.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- The preceding specification-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the terms and stated boundaries in the canonical documentation when adding or revising product claims.
- Use the exact canonical names in `docs/glossary.md` when naming Weavelit concepts.
- On first substantive use in a written section, format a canonical glossary term as a bold link to its glossary category; later uses in that section may be plain text.
- Update this inventory when repository-level assets, primary folders, or compatibility shims are added, removed, renamed, or moved.
