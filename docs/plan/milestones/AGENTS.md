# Roadmap Milestones Agent Guide

This directory holds the individual delivery-outcome records. Each milestone
is a checkable set of capabilities and protective boundaries needed to close a
delivery stage with confidence.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the independently maintained milestone goal documents.
- Each milestone describes the complete outcomes required to finish its phase;
  it does not own canonical product, security, or technical decisions.
- Canonical documents in `docs/` own settled decisions and definitions.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and maintenance rules for milestone documents.
- `milestone-1.md`: Core Server application delivery outcomes.
- `milestone-2.md`: TOTP MFA Module delivery outcomes.
- `milestone-3.md`: Web UI Client Module delivery outcomes.
- `milestone-4.md`: Zendesk Service Module delivery outcomes.
- `milestone-5.md`: Web UI delivery outcomes.
- `milestone-6.md`: Weavelit CLI Client Module delivery outcomes.
- `milestone-7.md`: Weavelit CLI delivery outcomes.
- `milestone-8.md`: MVP deployment packaging and verification outcomes.
- `milestone-9.md`: TechnitiumDNS Service Module delivery outcomes.
- `milestone-10.md`: User-associated Service Connection support outcomes.
- `milestone-11.md`: Automation Identity support outcomes.
- `milestone-12.md`: External Authentication support outcomes.
- `milestone-13.md`: Passkey MFA Module delivery outcomes.
- `milestone-14.md`: Server OCI image support outcomes.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`,
  and the repository-root `AGENTS.md`.
- Before creating or updating a production document, read the [Documentation Standards](../../documentation-standards.md) and apply its authority, document-type, lifecycle, structure, and writing rules.
- Keep changes focused on the affected milestone. When a milestone is added,
  removed, moved, or renamed, update the applicable parent asset inventories in
  the same change.
- Record every desired completion outcome, including capabilities, limits,
  protections, and safe failure or rejection behavior; do not replace those
  outcomes with an exhaustive implementation-task list.
- When a goal requires a settled product, security, or technical decision,
  update the canonical document or architecture decision record and keep the
  milestone aligned with that decision.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Preserve the required heading order and keep this guide under 100 lines.
- Name milestone documents `milestone-<number>.md` and title them
  `# Milestone <number>: <title>`.
- Keep each milestone's `## Goals` section after its title and its
  `## Related Documents` section at the end of the file.
- Keep goals as unchecked Markdown checklist items until their outcome is
  complete and verified.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
- Use the canonical names from `../../glossary.md` and format their first
  substantive use in a section as bold glossary links.
- Link to canonical documents rather than restating settled product, security,
  or technical decisions in a milestone.
