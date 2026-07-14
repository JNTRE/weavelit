# Roadmap Milestones Agent Guide

This directory holds the individual delivery-outcome records behind the phase
index in `docs/roadmap.md`. Each milestone turns a roadmap phase item into a
checkable set of capabilities and protective boundaries needed to close that
stage with confidence.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the independently maintained milestone goal documents
  linked from `../roadmap.md`.
- Each milestone describes the complete outcomes required to finish its phase;
  it does not own canonical product, security, or technical decisions.
- The parent roadmap owns phase order and the MVP boundary. Canonical documents
  in `docs/` own settled decisions and definitions.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local workflow, inventory, and maintenance rules for milestone documents.
- `milestone-1.md`: Core Server application delivery outcomes.
- `milestone-2.md`: TOTP MFA Module delivery outcomes.
- `milestone-3.md`: Web UI Client Module delivery outcomes.
- `milestone-4.md`: Zendesk Service Module delivery outcomes.
- `milestone-5.md`: Web UI delivery outcomes.
- `milestone-6.md`: Operations CLI Client Module delivery outcomes.
- `milestone-7.md`: Operations CLI delivery outcomes.
- `milestone-8.md`: MVP deployment packaging and verification outcomes.
- `milestone-9.md`: TechnitiumDNS Service Module delivery outcomes.
- `milestone-10.md`: User-associated Service Connection support outcomes.
- `milestone-11.md`: Automation Identity support outcomes.
- `milestone-12.md`: External Authentication support outcomes.
- `milestone-13.md`: Passkey MFA Module delivery outcomes.
- `milestone-14.md`: Server OCI image support outcomes.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `docs/AGENTS.md`, then the
  repository-root `AGENTS.md`.
- Read `../roadmap.md` to identify the milestone's phase and parent index entry
  before changing its goals.
- Keep changes focused on the affected milestone. When a milestone is added,
  removed, moved, or renamed, update `../roadmap.md` and the applicable parent
  asset inventories in the same change.
- Record every desired completion outcome, including capabilities, limits,
  protections, and safe failure or rejection behavior; do not replace those
  outcomes with an exhaustive implementation-task list.
- When a goal requires a settled product, security, or technical decision,
  update the canonical document or architecture decision record and keep the
  milestone aligned with that decision.

## Standards and Conventions

- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/sqlite/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Name milestone documents `milestone-<number>.md` and title them
  `# Milestone <number>: <title>`.
- Keep each milestone's `## Goals` section after its title and its
  `## Related Documents` section at the end of the file.
- Keep goals as unchecked Markdown checklist items until their outcome is
  complete and verified.
- Preserve a final `## Related Documents` section with non-numbered links to
  existing repository-relative canonical documents; update it when referenced
  documentation changes.
- Use the canonical names from `../glossary.md` and format their first
  substantive use in a section as bold glossary links.
- Link to canonical documents rather than restating settled product, security,
  or technical decisions in a milestone.
