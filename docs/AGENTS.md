# Docs Agent Guide

This directory is Weavelit's canonical record of the intended gateway: its vision, binding product and technical commitments, security constraints, terminology, unsettled design decisions, and product path. It guides changes to documented system boundaries rather than implementation work or release execution.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- `docs/` owns the canonical product and architecture documentation for Weavelit.
- This guide covers documentation workflow and document boundaries, not implementation-specific rules that do not yet exist in this repository.
- Child guides in `client-modules/`, `clients/`, `containers/`, `log-modules/`, `mfa-modules/`, `server/`, and `service-modules/` own their respective connection, client-application, container-image, log-storage and delivery, MFA-method, server-design, and provider-integration documentation boundaries; read the nearest applicable guide before editing.

## Asset Inventory

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
- `spec.md`: Technical specification and highest-level product and technical authority.
- `testing.md`: Cross-cutting test design, automated validation, deployment confidence, and agent test-authoring policy.
- `vision.md`: High-level intended product, system relationships, and links to the Technical Specification and Glossary.

## Working Rules

- Before editing, read the nearest `AGENTS.md`, then each parent guide upward to this file.
- Follow [Contribution Guidelines](CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- For changes under `docs/`, application documentation MUST comply with the [Documentation Standards](docs/documentation-standards.md), which govern document creation, document edits, file naming, and document organization.
- Use the exact canonical names in `docs/glossary.md` when a term is used and format terms as bold links on first substantive use in the text.
- For every implementation behavior change, add or update focused automated tests in the same change and run the applicable validation required by [the Testing and Validation Policy](docs/testing.md). Document any unavoidable manual verification in the owning documentation.
- Update this inventory when repository-level assets, primary folders, or compatibility shims are added, removed, renamed, or moved.
- Make minimal, targeted edits and MUST NOT perform broad rewrites unless explicitly requested.
- Record settled product, security, or technical commitments in `spec.md`, remove resolved items from `open-questions.md`, and place each decision in its appropriate canonical or design document.
