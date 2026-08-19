# Docs Agent Guide

This directory is Weavelit's canonical record of the intended gateway: its
vision, binding product and technical commitments, security constraints,
terminology, unsettled design decisions, and product path. It guides changes to
the system's documented boundaries rather than implementation work or release
execution.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- `docs/` owns the canonical product and architecture documentation for Weavelit.
- This guide covers documentation workflow and document boundaries, not implementation-specific rules that do not yet exist in this repository.
- Child guides in `client-modules/`, `clients/`, `containers/`, `log-modules/`, `mfa-modules/`, `server/`, and `service-modules/` own their respective connection, client-application, container-image, log-storage and delivery, MFA-method, server-design, and provider-integration documentation boundaries; read the nearest applicable guide before editing.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, workflow, and inventory rules for the canonical documentation set.
- `client-modules/`: Documentation for the server-side **[Client Modules](glossary.md#applications-and-interfaces)** that provide client-facing connection surfaces to the Weavelit Server.
- `clients/`: Documentation for individual client applications, including the **[Weavelit CLI](glossary.md#applications-and-interfaces)** and **[Web UI](glossary.md#applications-and-interfaces)**.
- `containers/`: Development and production OCI container-image documentation.
- `documentation-standards.md`: Shared authority, structure, and writing standards for application documentation under `docs/`.
- `glossary.md`: Canonical definitions for Weavelit applications, interfaces, identities, access, states, and requests.
- `log-modules/`: Documentation for server-side **[Log Modules](glossary.md#applications-and-interfaces)** that persist or deliver System Logs and Audit Logs.
- `mfa-modules/`: Documentation for server-side **[MFA Modules](glossary.md#applications-and-interfaces)** and their method-specific enrollment, verification, and protected factor-data handling.
- `open-questions.md`: Unresolved architecture and product decisions; resolved decisions belong in the Vision, Technical Specification, Glossary, or the relevant design document.
- `security-model.md`: Protected assets, trust assumptions, cross-cutting security invariants, and approved security profiles supporting the Technical Specification.
- `server/`: Implementation-design documentation for the **[Weavelit Server](glossary.md#applications-and-interfaces)**, including its API, authentication, authorization, **[Automation Identity](glossary.md#identities-and-access)**, audit, and observability boundaries.
- `service-modules/`: Documentation for **[Service Modules](glossary.md#applications-and-interfaces)** and their service-specific implementations, including Zendesk.
- `spec.md`: RFC 2119 technical specification and highest-level product and technical authority.
- `testing.md`: Cross-cutting test design, automated validation, deployment confidence, and agent test-authoring policy.
- `vision.md`: High-level intended product, system relationships, and links to the Technical Specification and Glossary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Before creating or updating application documentation, read the [Documentation Standards](documentation-standards.md) and apply its authority, structure, and writing rules.
- Make minimal, targeted edits; avoid broad rewrites unless explicitly requested.
- Use `glossary.md` for canonical terms and keep their usage consistent across the documentation.
- Record settled product, security, or technical commitments in `spec.md`; remove a resolved item from `open-questions.md` and place its decision in the appropriate canonical or design document.
  security, and technical decisions instead of redefining those decisions.
- Keep security constraints in `security-model.md` aligned with `spec.md`, and preserve each document's stated scope.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Use the exact canonical names defined in `glossary.md` when documenting Weavelit concepts.
- On first substantive use in each document section, write a canonical glossary term
  as a bold link to its glossary category; later uses in that section may be plain
  text.
- Any `AGENTS.md` created under `docs/` must keep Related Documents maintenance requirements integrated as bullets in `Standards and Conventions`.
- Every production document must include a `## Related Documents` section at the end of the document.
- `Related Documents` entries must use non-numbered Markdown link bullets in this format: `[Description](path)`.
- Include only valid, repository-relative links to existing canonical documents.
- Update `Related Documents` in the same change whenever files are added, moved, renamed, replaced, or retired.
- Remove stale links and add canonical links so the section reflects current source-of-truth references.
- Keep intended product and technical commitments in `spec.md`; do not leave resolved decisions in `open-questions.md`.
- Do not restate a canonical decision in multiple documents when a link to its owning document preserves the needed context.
