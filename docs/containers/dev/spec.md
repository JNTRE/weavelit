# Development Container Specification

## Purpose

The development container provides a reproducible OCI-compatible environment to
build, run, test, and restart the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
without requiring Rust on the host. Docker is a supported local client for this
image contract; the image and runtime contract must not depend on Docker-only
behavior.

## Current Boundary

The development image is reserved for Milestone 1. Its Containerfile remains a
non-runnable placeholder until the Server defines its development
configuration, persistent-state location, bootstrap secret-file interface, and
startup behavior.

When implemented, the development image must:

- target Ubuntu 26.04 LTS `amd64` and use a pinned base-image digest;
- install the Rust version and quality-gate components declared by
  `server/rust-toolchain.toml`;
- run as a non-root development user;
- use a mounted Server source tree and run `make check` for the complete Rust
  quality-gate suite;
- keep non-secret development configuration outside the image and receive it
  through environment variables;
- mount bootstrap secrets as read-only local files, never include them in the
  build context, image layers, or environment variables; and
- use explicitly managed volumes for future persistent Server state and any
  optional build-cache data.

## Validation

The implemented image must be built and exercised with both a Docker command
and at least one OCI-compatible alternative such as Podman or Buildah. Its
validation must run `make check` inside the container and confirm that source,
state, and secret mounts follow this specification.

## Related Documents

- [Testing and Validation Policy](../../testing.md)
- [Milestone 1](../../roadmap/milestone-1.md)
- [Production Container Specification](../prod/spec.md)
- [Open Questions](../../open-questions.md)
