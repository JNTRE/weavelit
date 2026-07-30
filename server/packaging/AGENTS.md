# Server Packaging Agent Guide

This directory is reserved for assets that package the Weavelit Server for
installation. The Server package includes the Server executable, Web UI assets,
and Admin CLI; it does not install source code or development tooling on the
supported host.

## Purpose and Scope

Use this section to understand what this directory owns, what it does not own, and where child paths own detailed rules.

- This directory owns the Server package layout and release-artifact boundaries.
- It does not own Server or Web UI source, application configuration, Init state, or Service Connection setup.
- Child paths own platform-specific release packaging assets.

## Asset Inventory

Use this section as the source of truth for what assets belong in this directory and what each asset is for.

- `AGENTS.md`: Local routing, inventory, and Server packaging-boundary rules.
- `deb/`: Debian package asset boundary for the Server release.

## Usage Guidance

Follow this section for workflow, sequencing, and decision order when making changes in this directory.

- Before editing, read the nearest `AGENTS.md`, then `../AGENTS.md`, and the repository-root `AGENTS.md`.
- Read the authoritative [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8) and the relevant Server requirements before changing package behavior.
- Keep packaging assets separate from application source and test package behavior in a production-like environment when packaging is introduced.
- Record release build, installation, initialization, verification, and troubleshooting instructions with the packaged workflow.

## Standards and Conventions

Treat every rule in this section as mandatory for formatting, naming, scope boundaries, and consistency.

- Update this `AGENTS.md` asset inventory whenever relevant directory assets change.
- Documentation is AI-maintained: agents must keep it accurate, complete, logically structured, and located in the appropriate documentation boundary.
- Every change must include an update to its relevant documentation under `docs/` in the same change.
- Reorganize, move, add, or remove documentation as needed when a change makes the current structure unclear, duplicates information, or places information outside its owning document.
- Keep documentation focused and navigable. When a document grows broad, difficult to navigate, or mixes distinct concerns, split it into focused, appropriately named documents and organize them within `docs/`.
- The preceding documentation-maintenance requirement must appear verbatim in every `AGENTS.md` in this repository.
- Package the Server, its Web UI assets, and the Admin CLI as one Server release artifact.
- Do not make package installation create application users, configure Service Connections, complete Init, or start normal Server operation against uninitialized state.
- Keep platform-specific package files in their named child boundary.
