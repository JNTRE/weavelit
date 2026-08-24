# Development Container Design

This document defines the development OCI image contract for the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**. It is authoritative for the development image's local build, test, launch, and validation boundaries, while the production OCI image remains a separate contract.

## Represented Areas

| Type | Link |
| --- | --- |
| Folder | [Development container implementation](../../../server/containers/dev/) |
| Containerfile | [Development Containerfile](../../../server/containers/dev/Containerfile) |
| Launcher lifecycle preflight | [Lifecycle harness](../../../server/containers/dev/run-local-server-lifecycle-test.sh) |
| Validation policy | [Testing and Validation Policy](../../testing.md) |

## Purpose

The development container provides a reproducible OCI-compatible environment to
build, run, test, and restart the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
without requiring Rust or Node.js on the host. Docker is a supported local
client for this image contract; the image and runtime contract must not depend
on Docker-only behavior.

## Scope And Exclusions

This document owns the development image and its local validation contract. It
does not define the Server application lifecycle, the production OCI image, or
deployment behavior.

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

## Local Docker Workflow

The host editor remains outside the container. Source edits, repository tooling,
and editor extensions therefore run on the host, while Rust, Node.js, browser,
and Server commands run in the development image through these targets:

| Target | Purpose |
| --- | --- |
| `make -C server container-check` | Runs the launcher lifecycle preflight on the host through `sh`, then builds the disposable development image and runs the complete `make check` gate in it. The host preflight requires the standard POSIX shell and coreutils. |
| `make -C server container-shell` | Opens an interactive shell in the development image for targeted Linux commands. |
| `make -C server container-run` | Builds the Web UI and release Server, then starts a named local Server container for browser testing. |
| `make -C server container-stop` | Stops the named local Server container. |
| `make -C server container-logs` | Follows the named local Server container's output. |

The targets mount the repository at `/workspace` and create named Docker
volumes for the Rust registry, Rust Git dependencies, npm cache, Server target
directory, and Web UI dependencies. The containers run as the image's
non-root `weavelit` user. Docker initializes these volumes for that user; host
tooling must not write build outputs directly.

`container-check` uses only the source and build-cache volumes. It creates no
retained Server state, so it cannot change the state used by manual browser
testing. Contributors use this target for the required local `dev` integration
gate. `dev` has no GitHub gate.

`container-run` additionally uses a named, owner-only Server state volume. It
starts the Server on its required container-loopback listener and publishes an
internal relay only as `127.0.0.1:8443` on the host. An operator can open
`https://localhost:8443` in a host browser; the self-signed local certificate
will require the normal browser warning. Docker does not expose that published
port to the local network. The persistent state volume survives
`container-stop` and a later `container-run`; remove it deliberately with
`docker volume rm weavelit-server-local-state` only when a fresh local
deployment is intended.

### Local TLS Material

The `run-local-server` launcher creates its self-signed certificate and private
key in an owner-only temporary directory. That directory is ephemeral: the
launcher removes it when OpenSSL certificate generation fails, when the Server
exits, and when the launcher receives a termination signal. It MUST NOT emit
the temporary-directory path, certificate path, private-key path, or private
key material in diagnostics. Cleanup MUST retain the exit status that caused
it to run.

The Docker commands use no host networking. This keeps the workflow portable
to Docker Desktop on macOS, where Linux-style host networking is unavailable,
and lets the Server retain its loopback-only listener policy.

## Validation

The implemented image must be built and exercised with the documented Docker
targets for Milestone 1 local validation. Before image or container build,
`container-check` must run the launcher lifecycle preflight on the host through
`sh`; this preflight assumes the host provides the standard POSIX shell and
coreutils. Each controlled launcher case has a five-second watchdog. On timeout,
the harness returns status `124`, sends `SIGTERM` to the launcher, waits three
seconds (60 0.05-second intervals) for launcher-owned cleanup, sends `SIGKILL` to any remaining recorded
relay or Server child so the launcher can reap it, then sends `SIGKILL` to the
launcher only when it still remains alive. The timeout regression keeps a fake
Server in its wait mode and proves that launcher cleanup terminates the relay
and Server while removing its temporary TLS material. The preflight must then
build the disposable development image, run `make check` inside it, and confirm
that source and build-cache mounts follow this design.
`container-run` must confirm that the named state-root volume persists across
container stop and restart boundaries and that the Web UI is reachable only
through the host-loopback published port.

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
