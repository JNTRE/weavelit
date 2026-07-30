# Planning Documentation Agent Guide

This directory organizes Weavelit's delivery-planning documentation. It keeps
milestone navigation, the high-level issue view, and GitHub Project workflow
references in a dedicated planning boundary.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns repository documentation organized around delivery planning.
- It does not own canonical product, security, or technical decisions; those remain in the top-level documents under `docs/`.
- GitHub Milestones own milestone titles, summaries, goals, state, dates, progress, and assigned issues.
- The `issues/` and `milestones/` child directories own the open-issue overview and the repository milestone navigation index, respectively.
- The `project/` child directory owns the reference for active GitHub issue metadata and Project workflow values.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and planning-documentation boundary rules.
- `issues/`: High-level documentation of open GitHub issues and their project-planning metadata.
- `milestones/`: Repository navigation to authoritative GitHub Milestones and their summaries; it does not duplicate milestone goals or state.
- `project/`: GitHub issue-metadata and Project workflow standards; follow `project/AGENTS.md` before editing this boundary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the
  [Documentation Standards](../documentation-standards.md) and apply its
  authority, document-type, lifecycle, structure, and writing rules.
- Read `issues/issues.md` before changing how open work is summarized or mapped to GitHub Project metadata.
- Read `milestones/milestones.md`, then the linked authoritative GitHub Milestone, before changing a milestone. Keep GitHub Milestone outcomes aligned with canonical documents for settled product, security, and technical decisions; unresolved choices remain in [Open Questions](../open-questions.md).
- Read `project/AGENTS.md` and `project/project-standards.md` before changing
  documented GitHub Issue or Pull Request metadata or Project workflow values.
- Keep planning outcomes aligned with canonical documents for settled product, security, and technical decisions instead of redefining those decisions.
- Keep repository milestone navigation in `milestones/`; do not create local per-milestone outcome documents.

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
