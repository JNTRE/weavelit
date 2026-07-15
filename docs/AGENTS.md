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
- Child guides in `client-modules/`, `clients/`, `containers/`, `log-modules/`, `mfa-modules/`, `plan/`, `server/`, and `service-modules/` own their respective connection, client-application, container-image, log-storage and delivery, MFA-method, planning, server-design, and provider-integration documentation boundaries; read the nearest applicable guide before editing.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, workflow, and inventory rules for the canonical documentation set.
- `client-modules/`: Documentation for the server-side **[Client Modules](glossary.md#applications-and-interfaces)** that provide client-facing connection surfaces to the Weavelit Server.
- `clients/`: Documentation for individual client applications, including the **[Weavelit CLI](glossary.md#applications-and-interfaces)** and **[Web UI](glossary.md#applications-and-interfaces)**.
- `containers/`: Development and production OCI container-image specifications.
- `core-statements.md`: Current product, security, and technical truths; expand or replace statements only after a clear decision.
- `glossary.md`: Canonical definitions for Weavelit applications, interfaces, identities, access, states, and requests.
- `log-modules/`: Documentation for server-side **[Log Modules](glossary.md#applications-and-interfaces)** that persist or deliver System Logs and Audit Logs.
- `mfa-modules/`: Documentation for server-side **[MFA Modules](glossary.md#applications-and-interfaces)** and their method-specific enrollment, verification, and protected factor-data handling.
- `open-questions.md`: Unresolved architecture and product decisions; resolved decisions belong in the Vision, Core Statements, Glossary, or an architecture decision record.
- `plan/`: Planning documentation, including individually maintained milestone outcome documents indexed by `roadmap.md`.
- `roadmap.md`: Delivery-phase index and completion guidance for the milestone documents; canonical documents supply the product, security, and technical direction for their goals.
- `security-model.md`: Security requirements and implementation constraints supporting the Core Statements, not a complete implementation design.
- `server/`: Implementation-design documentation for the **[Weavelit Server](glossary.md#applications-and-interfaces)**, including its API, authentication, authorization, **[Automation Identity](glossary.md#identities-and-access)**, audit, and observability boundaries.
- `service-modules/`: Documentation for **[Service Modules](glossary.md#applications-and-interfaces)** and their service-specific implementations, including Zendesk.
- `testing.md`: Cross-cutting test design, automated validation, deployment confidence, and agent test-authoring policy.
- `vision.md`: High-level intended product, system relationships, and links to the Core Statements and Glossary.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then the repository-root `AGENTS.md`.
- Make minimal, targeted edits; avoid broad rewrites unless explicitly requested.
- Use `glossary.md` for canonical terms and keep their usage consistent across the documentation.
- Record settled product, security, or technical commitments in `core-statements.md`; remove a resolved item from `open-questions.md` and place its decision in the appropriate canonical document or an architecture decision record.
- Keep roadmap milestones aligned with canonical documents for settled product,
  security, and technical decisions instead of redefining those decisions.
- Keep security constraints in `security-model.md` aligned with `core-statements.md`, and preserve each document's stated scope.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Preserve the required heading order and keep this guide under 100 lines.
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
- Keep intended product and technical commitments in `core-statements.md`; do not leave resolved decisions in `open-questions.md`.
- Do not restate a canonical decision in multiple documents when a link to its owning document preserves the needed context.
