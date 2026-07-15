# Server Debian Packaging Agent Guide

This directory is reserved for the Debian package assets that install the
Weavelit Server on Ubuntu 26.04 LTS `amd64`. The resulting versioned package
includes the Server, Web UI assets, and Admin CLI without requiring Rust,
source code, or development tooling on the host.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns Debian-specific Server package files and installation behavior.
- It does not own Server source, Web UI source, runtime initialization, or Service Connection configuration.
- Future child paths own only narrower Debian packaging guidance that differs from this boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Debian Server packaging-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../docs/plan/milestones/milestone-8.md` and the relevant Server requirements before changing Debian package behavior.
- Keep package installation, service definition, non-secret configuration, persistent-state, and log-location behavior here.
- Verify release artifacts in a clean, production-like Ubuntu environment when this packaging workflow is introduced.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Install the Server package's service definition and its required non-secret configuration, persistent-state, and log locations.
- Do not create application users, configure Service Connections, complete Init, or start normal operation during package installation.
- Keep Debian packaging behavior aligned with the documented Ubuntu 26.04 LTS `amd64` support requirement.