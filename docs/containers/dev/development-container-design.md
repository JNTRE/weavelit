# Development Container Design

## Purpose

The development container provides a reproducible OCI-compatible environment to
build, run, test, and restart the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
without requiring Rust on the host. Docker is a supported local client for this
image contract; the image and runtime contract must not depend on Docker-only
behavior.

## Image Contract

The development image must:

- target Ubuntu 26.04 LTS (`linux/amd64` and `linux/arm64`) and use a pinned multi-arch manifest digest;
- install the Rust version and quality-gate components declared by
  `server/rust-toolchain.toml`;
- install the Node.js runtime and bundled npm release declared by
  `server/web-ui/.node-version`, verified against the published upstream
  checksums for the build architecture, so the image can run the Web UI
  install, typecheck, unit-test, production-build, and bundle-inventory stages
  of `make check`;
- install the Playwright Chromium build keyed to the `@playwright/test` release
  resolved by `server/web-ui/package-lock.json`, together with the Ubuntu shared
  libraries that browser requires and the `openssl` command the browser test
  fixture uses to generate short-lived TLS material, so the image can run the
  Web UI browser smoke test stage of `make check`;
- provide `git` and the GitHub CLI (`gh`) for repository workflows; GitHub
  authentication must be supplied at runtime through persistent configuration
  outside the image and never embedded in the image;
- run as a non-root development user;
- use a mounted Server source tree and run `make check` for the complete Web UI
  and Rust quality-gate suite;
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
Containerfile, mount the source tree at `/workspace`, declare named Docker
volumes for the state root path exposed through `WEAVELIT_STATE_ROOT` and the
GitHub CLI configuration path `/home/weavelit/.config/gh`, and require
`rust-lang.rust-analyzer` as the minimum VS Code extension. It must use host
networking (`--network=host`) so the Server's configured HTTPS port is directly
reachable from the development host's browser and other applications. On Linux
hosts, this works seamlessly with VS Code Dev Containers. It does not expose
the listener to other devices on the local network; the Server's
trusted-listener policy permits only loopback addresses. The named GitHub CLI
configuration volume preserves runtime-only authentication across Dev Container
rebuilds and restarts; credentials must not appear in the repository, image,
build arguments, or environment files. Both volume roots must be initialized
with mode `0700` and use the host UID and GID on Linux, where Dev Containers
synchronizes the `weavelit` account to keep bind mounts writable; on
non-Linux hosts they must retain the image account ownership of `10001:10001`.

## Validation

The implemented image must be built and exercised with Docker commands for
Milestone 1 local validation. Its validation must run `make check` inside the
container and confirm that source, state, and secret mounts follow this design.
Validation must also confirm that the named state-root volume persists across
container stop, rebuild, and restart boundaries.

Browser-based end-to-end validation is part of the image contract. The image
installs the pinned Chromium build as the non-root development user, so
`make check` runs the Web UI browser smoke test against the release Server
binary without installing root-level packages during validation.

## Related Documents

- [Testing and Validation Policy](../../testing.md)
- [Server Lifecycle Design](../../server/lifecycle/lifecycle-design.md)
- [Server Init Design](../../server/lifecycle/init/init-design.md)
- [Server Restore Design](../../server/lifecycle/restore/restore-design.md)
- [Production Container Design](../prod/production-container-design.md)
- [Open Questions](../../open-questions.md)
