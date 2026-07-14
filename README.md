# Weavelit

An AI agent service gateway

## Implementation Layout

The repository contains two separately packaged applications. The `server/`
directory owns the Weavelit Server, its compiled-in modules, the Admin CLI, the
Web UI source, tests, and Debian packaging. The `operations-cli/` directory
owns the separately packaged macOS client application.

- `server/crates/`: Rust Server, Admin CLI, Application Database backend, and compiled-in Client, MFA, Log, and Service Module crates.
- `server/web-ui/`: Web UI source built into the Server package.
- `server/tests/`: Server-focused integration and end-to-end tests.
- `server/packaging/deb/`: Debian package assets for the Server release.
- `operations-cli/src/`: Operations CLI source.
- `operations-cli/tests/`: Operations CLI tests.
- `operations-cli/packaging/macos/`: macOS `arm64` release packaging assets.
