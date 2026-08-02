# Development Container Design

## Purpose

The development container provides a reproducible OCI-compatible environment to
build, run, test, and restart the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
without requiring Rust on the host. Docker is a supported local client for this
image contract; the image and runtime contract must not depend on Docker-only
behavior.

## Current Boundary

The development image is reserved for Milestone 1. Its Containerfile remains a
non-runnable placeholder until the Server defines its development
configuration, protected persistent-state location, database-locator
persistence, and startup behavior.

The placeholder's `org.opencontainers.image.description` label points to this
document.

When implemented, the development image must:

- target Ubuntu 26.04 LTS (`linux/amd64` and `linux/arm64`) and use a pinned multi-arch manifest digest;
- install the Rust version and quality-gate components declared by
  `server/rust-toolchain.toml`;
- run as a non-root development user;
- use a mounted Server source tree and run `make check` for the complete Rust
  quality-gate suite;
- keep non-secret development configuration outside the image and receive it
  through environment variables;
- persist the Server-owned deployment record and
  **[Application Database](../../glossary.md#applications-and-interfaces)**
  locator, encrypted database connection values, and Server-managed database
  files together in explicitly managed state without exposing a client-selected
  storage path, and never include plaintext secrets in the build context, image
  layers, or environment variables;
- start an unconfigured Server in restricted pre-operational mode, leave
  Application Database selection to the shared lifecycle contract, and leave
  fresh or restored application state to a
  **[Client Module](../../glossary.md#applications-and-interfaces)** with the
  corresponding Init or Restore capability; and
- use explicitly managed volumes for future persistent Server state and any
  optional build-cache data.

The repository-level `.devcontainer/devcontainer.json` must reference this
Containerfile, mount the source tree at `/workspace`, declare a named Docker
volume for the state root path exposed through `WEAVELIT_STATE_ROOT`, and
require `rust-lang.rust-analyzer` as the minimum VS Code extension.

When using VS Code workspaces with this development container, contributors
must open the repository folder itself or the repository-local
`weavelit.code-workspace` file so the container mounts this repository rather
than a parent directory.

## Validation

The implemented image must be built and exercised with Docker commands for
Milestone 1 local validation. Its validation must run `make check` inside the
container and confirm that source, state, and secret mounts follow this design.
Validation must also confirm that the named state-root volume persists across
container stop, rebuild, and restart boundaries.

## Related Documents

- [Testing and Validation Policy](../../testing.md)
- [Milestone 1](../../plan/milestones/milestones.md#milestone-1-core-server-application)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Init Design](../../server/lifecycle/init/init-design.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Production Container Design](../prod/production-container-design.md)
- [Open Questions](../../open-questions.md)
