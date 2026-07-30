# Milestone 8: MVP Package Build and Validation

## GitHub Milestone

Implementation progress is tracked in [GitHub Milestone 8](https://github.com/JNTRE/weavelit/milestone/8). Keep that GitHub Milestone aligned with this canonical document when this milestone's title, goals, or scope changes.

## Goals

- [ ] A versioned `.deb` package installs the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**, **[Web UI](../../glossary.md#applications-and-interfaces)** assets, and **[Admin CLI](../../glossary.md#applications-and-interfaces)** on Ubuntu 26.04 LTS `amd64` without requiring Rust, source code, or development tooling on the host.
- [ ] The Server package installs the service definition and the non-secret configuration, persistent-state, and log locations required by the Server. Package installation does not create application users, configure **[Service Connections](../../glossary.md#applications-and-interfaces)**, complete **[Init](../../glossary.md#states-and-requests)**, or start normal Server operation against uninitialized state.
- [ ] A **[Host Administrator](../../glossary.md#identities-and-access)** can install the Server package on a clean supported Ubuntu host, complete Init interactively or through the defined non-interactive bootstrap configuration, start the Server service, and reach the configured HTTPS listener.
- [ ] A versioned **[Weavelit CLI](../../glossary.md#applications-and-interfaces)** artifact for macOS 26 and later on Apple Silicon (`arm64`) can be installed without Rust, source code, or provider credentials.
- [ ] An installed Weavelit CLI can authenticate to the installed Weavelit Server and invoke a permitted supported **[Operation](../../glossary.md#applications-and-interfaces)** through `/api/v1/`.
- [ ] The release artifacts and their supported platform requirements have documented build, installation, initialization, verification, and troubleshooting instructions.

## Related Documents

- [Vision](../../vision.md)
- [Core Statements](../../core-statements.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Open Questions](../../open-questions.md)
- [Testing and Validation Policy](../../testing.md)
