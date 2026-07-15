# Planning Documentation Agent Guide

This directory organizes Weavelit's delivery-planning documentation. It keeps
the delivery-phase index, milestone outcomes, and the high-level issue view in
a dedicated planning boundary.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns documentation organized around delivery planning and milestone outcomes.
- It does not own canonical product, security, or technical decisions; those remain in the top-level documents under `docs/`.
- The `issues/` and `milestones/` child directories own the open-issue overview and milestone documents, respectively.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and planning-documentation boundary rules.
- `issues/`: High-level documentation of open GitHub issues and their project-planning metadata.
- `milestones/`: Individually maintained milestone outcome documents. Each records every desired capability, limit, protection, and safe failure or rejection behavior required to complete its delivery stage; a milestone is complete only when every recorded outcome is implemented and verified according to the [Testing and Validation Policy](../testing.md).

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `issues/issues.md` before changing how open work is summarized or mapped to GitHub Project metadata.
- Read the affected document in `milestones/` before changing its outcomes. Keep milestones aligned with canonical documents for settled product, security, and technical decisions; unresolved choices remain in [Open Questions](../open-questions.md).
- Keep planning outcomes aligned with canonical documents for settled product, security, and technical decisions instead of redefining those decisions.
- Place milestone-specific guidance and outcome documents in `milestones/`.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep the required heading order and keep this guide under 100 lines.
- Use exact canonical names from `../glossary.md`; on first substantive use in a section, format a canonical term as a bold link to its glossary category.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
