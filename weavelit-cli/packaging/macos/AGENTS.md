# Weavelit CLI macOS Packaging Agent Guide

This directory is reserved for assets that package the versioned Weavelit CLI
artifact for macOS 26 and later on Apple Silicon (`arm64`). The installed client
must work without Rust, source code, or provider credentials and authenticate to
a compatible installed Weavelit Server through its versioned HTTPS interface.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns macOS `arm64` Weavelit CLI packaging and installation behavior.
- It does not own Weavelit CLI source, Server packaging, provider credentials, or Server configuration and initialization.
- Future child paths own only narrower macOS packaging guidance that differs from this boundary.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and macOS Weavelit CLI packaging-boundary rules.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read this `AGENTS.md`, then `../AGENTS.md`, `../../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read `../../../docs/plan/roadmap/milestone-8.md` and the Weavelit CLI requirements before changing macOS release behavior.
- Keep macOS installation behavior here and verify an installed artifact against an installed compatible Server when this workflow is introduced.
- Record macOS build, installation, verification, and troubleshooting instructions with the release workflow.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Every change must include an update to its relevant documentation. For feature-specific work, update the feature's `spec.md` under `docs/` (for example, `docs/server/database/spec.md`) in the same change.
- Specification documents are AI-maintained documentation: agents must keep them accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Reorganize, move, add, or remove specification content as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Do not allow a specification document to become a monolith; split large documents into focused sibling documents named `<name>-spec.md` when doing so improves logical structure, navigation, or maintainability.
- Keep this artifact limited to macOS 26 and later on Apple Silicon (`arm64`) until support for another platform is recorded.
- Do not require Rust, source code, or provider credentials to install the released CLI artifact.
- Verify installed-client authentication and permitted Operation invocation against the versioned Server interface when packaging is introduced.
