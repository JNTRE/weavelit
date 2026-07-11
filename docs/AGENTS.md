# Docs Agent Guide

This directory is Weavelit's canonical record of the intended gateway: its
vision, binding product and technical commitments, security constraints,
terminology, and unsettled design decisions. It guides changes to the system's
documented boundaries rather than implementation work or release planning.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- `docs/` owns the canonical product and architecture documentation for Weavelit.
- This guide covers documentation workflow and document boundaries, not implementation-specific rules that do not yet exist in this repository.
- No child paths currently own more detailed guidance; add a child guide only when a distinct local workflow or policy boundary emerges.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, workflow, and inventory rules for the canonical documentation set.
- `core-statements.md`: Current product, security, and technical truths; expand or replace statements only after a clear decision.
- `glossary.md`: Canonical definitions for Weavelit applications, interfaces, identities, access, states, and requests.
- `open-questions.md`: Unresolved architecture and product decisions; resolved decisions belong in the Vision, Core Statements, Glossary, or an architecture decision record.
- `security-model.md`: Security requirements and implementation constraints supporting the Core Statements, not a complete implementation design.
- `vision.md`: High-level intended product, system relationships, and links to the Core Statements and Glossary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Make minimal, targeted edits; avoid broad rewrites unless explicitly requested.
- Use `glossary.md` for canonical terms and keep their usage consistent across the documentation.
- Record settled product, security, or technical commitments in `core-statements.md`; remove a resolved item from `open-questions.md` and place its decision in the appropriate canonical document or an architecture decision record.
- Keep security constraints in `security-model.md` aligned with `core-statements.md`, and preserve each document's stated scope.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Preserve the required heading order and keep this guide under 100 lines.
- Use the exact canonical names defined in `glossary.md` when documenting Weavelit concepts.
- Keep intended product and technical commitments in `core-statements.md`; do not leave resolved decisions in `open-questions.md`.
- Do not restate a canonical decision in multiple documents when a link to its owning document preserves the needed context.