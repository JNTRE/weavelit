# Weavelit Server Agent Guide

This folder documents the implementation-design boundaries of the **[Weavelit Server](../glossary.md#applications-and-interfaces)**. It routes detailed work on the Server's API, access controls, **[Audit Logs](../glossary.md#applications-and-interfaces)**, and observability to focused child directories while keeping product commitments in the canonical top-level documents.

## Instruction Precedence

Apply instructions in this order:

1. Nearest folder-level `AGENTS.md` in the path being edited.
2. Repository root `AGENTS.md`.
3. Tool-specific overlays for runtime behavior only.

## Purpose and Scope

- This directory owns shared **[Weavelit Server](../glossary.md#applications-and-interfaces)** implementation-design documentation and routing to server-boundary documentation.
- It does not own product commitments, security requirements, or unresolved decisions; those remain in `../spec.md`, `../security-model.md`, and `../open-questions.md`.
- The `api/`, `authentication/`, `authorization/`, `automation-identities/`, `audit/`, `database/`, `lifecycle/`, `observability/`, and `user-stories/` child directories own detailed documentation for their respective Server boundaries and user-visible lifecycle workflows.

## Asset Inventory

- `api/`: Documentation for the Server's normal authenticated HTTPS application interface and shared API contract; restricted pre-operational lifecycle and workflow design remain in their named Server documents.
- `authentication/`: Documentation for the Server's human authentication and Automation Identity credential-validation design.
- `authorization/`: Documentation for the Server's permission and policy-evaluation design.
- `automation-identities/`: Documentation for **[Automation Identity](../glossary.md#identities-and-access)** lifecycle, ownership, and accountability design.
- `audit/`: Documentation for the Server's accountability and Audit Log design.
- `database/`: Documentation for Application Database backend boundaries and their implementation design.
- `lifecycle/`: Shared pre-operational lifecycle design, with focused child boundaries for the Server-owned Init and Restore workflows.
- `observability/`: Documentation for Server System Log design and future operational diagnosis.
- `server-architecture-design.md`: Shared Server workspace, crate-composition, and lifecycle design rules.
- `user-stories/`: User-visible Web UI Init and Restore workflow narratives; Server contract and persistence design remain in the parent documents.

## Working Rules

- MUST read the nearest `AGENTS.md`, then `../AGENTS.md`, then the repository root `AGENTS.md` before editing.
- MUST follow [Contribution Guidelines](../../CONTRIBUTING.md) for branch, commit, and pull-request workflow, naming, and message requirements.
- Documentation changes under `docs/` MUST comply with the [Documentation Standards](../documentation-standards.md).
- MUST use the exact canonical names in [the glossary](../glossary.md) and format a term as a bold glossary link on its first substantive use.
- MUST update this inventory when local assets or routing directories are added, removed, renamed, or moved.
- MUST keep broad shared Server design documentation directly in this folder; place pre-operational lifecycle design, boundary-specific detail, and user-visible lifecycle narratives in their appropriate child directories.
- MUST update `../spec.md` for settled commitments and `../open-questions.md` for unresolved choices instead of treating local design documentation as their replacement.

- MUST keep provider-integration detail in `../service-modules/` and client-connection detail in `../client-modules/`.
