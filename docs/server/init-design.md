# Server Init Design

This document defines the Server-owned implementation boundary for
**[Init](../glossary.md#states-and-requests)**. It makes interactive,
host-local bootstrap, and future container bootstrap adapters invoke one
initialization workflow while keeping Server validation, secret handling, and
state changes outside those adapters.

## Crate And Adapter Boundary

`weavelit-server-init` is the dedicated Server-owned crate for initialization.
It exposes one named `InitializeServer` use case and the normalized in-memory
request that it accepts. It owns bootstrap configuration processing,
secret-file handling, Server semantic validation, recovery-key delivery, and
the atomic initial-state transition.

Before input collection, an adapter calls the crate's non-mutating
`PreflightInitializeServer` helper with the selected backend and the minimum
connection configuration needed to open it. It returns an opaque preflight
handle only when the target is eligible for Init. The handle is bound to the
selected Application Database and is supplied to `InitializeServer`, which
rechecks eligibility during its final atomic state transition. Preflight is not
an alternative initialization path and cannot create or modify application
state.

The normal `weavelit-server` runtime does not depend on
`weavelit-server-init` and does not expose an Init interface. It opens existing
application state for normal operation and does not start normally when that
state is absent or invalid.

The **[Admin CLI](../glossary.md#applications-and-interfaces)** is a host-local
adapter. It owns command parsing, interactive prompts, and presentation of
normalized results. It has no direct Application Database or driver access and
cannot create an alternative initialization path. Interactive prompts and
non-interactive bootstrap both first use Server-owned preflight, then create the
same normalized request and invoke `InitializeServer`.

A future container bootstrap adapter passes its mounted bootstrap-configuration
path to `weavelit-server-init`. It reuses the same parser, secret-file rules,
and use case without depending on Admin CLI presentation code.

## Bootstrap Configuration

Non-interactive bootstrap uses a human-authored TOML file. Its top-level
`format_version` field is required. Unsupported versions fail safely rather
than being inferred or partially interpreted.

The configuration contains non-sensitive operational values and typed
secret-file references only. Secret-bearing fields are represented by
`SecretFileReference` values, exposed as TOML `*_file` fields. Inline secret
values, secret encodings, and environment-variable interpolation are rejected.

The initial Administrator password, Log Module credentials, future Application
Database credentials, and any host-supplied private keys or certificates are
secret-bearing values. The generated backup recovery private key is not
bootstrap input.

`weavelit-server-init` owns bootstrap schema parsing, structural validation,
secret-file reference handling, and conversion to the normalized request. It
never reads bootstrap secrets from environment variables. Unrelated non-secret
environment configuration remains valid; a future explicitly named secret
environment variable is unsupported and must be rejected rather than read.

Preflight reads only the selected backend's non-sensitive configuration. When a
future backend cannot be opened without a secret, the Init crate may read only
the referenced connection secret needed for that opening; the adapter never
reads it. For the MVP SQLite backend, preflight needs no secret. After preflight
accepts the target, the adapter may collect the initial Administrator password,
Log Module credentials, and other initialization secrets. An already initialized
target therefore rejects before those values are prompted for or read.

## Secret Files And Recovery-Key Delivery

The Init crate accepts a secret file only when the opened object is a
bounded-size regular non-symlink file with no group or world access. It
defensively verifies the opened file, reads UTF-8 content once, trims at most
one final newline, and never logs the secret, its contents, or its path.

The shared rule does not require a specific file owner. In a future production
container, the Server and any bootstrap adapter run as a dedicated non-root
service user; mounted secret files are readable only by that user. Packaged
host deployments leave service-account and file-owner choices to the
**[Host Administrator](../glossary.md#identities-and-access)**, provided the
Server process can read the file and it is not group- or world-accessible.

During Init, the Server generates a backup recovery key pair and retains only
the public key. Interactive Init may display the private key once after an
explicit confirmation. Non-interactive bootstrap requires an explicit output
file and never prints or logs the private key. Future container bootstrap uses
the same mounted output-file mechanism.

Recovery-key delivery and initialization use a recoverable two-phase protocol:

1. The Init crate creates a restrictive, unique staging file beside the requested
   output path, writes and synchronizes the private key, and records an
   `InitializationPending` checkpoint containing only the public key and a
   delivery nonce. Normal Server startup treats this checkpoint as uninitialized.
2. It finalizes the staging file at the requested output path without replacing
   an existing file, then atomically replaces the checkpoint with the complete
   initialized state.

If a delivery or final-state write fails, the checkpoint prevents a new key pair
from being generated. A retry using the same output path verifies the staged or
final key against the pending public key and resumes the same `InitializeServer`
workflow. If neither delivery file remains, the crate safely discards the
non-operational checkpoint before beginning a new attempt. It never marks the
Server initialized before private-key delivery succeeds, and it never overwrites
an unrelated output file.

## Initialization Lifecycle And Errors

`PreflightInitializeServer` identifies and opens the configured
**[Application Database](../glossary.md#applications-and-interfaces)** before
interactive initialization secrets are collected or read. If the database is
already initialized, it returns `AlreadyInitialized` without changing state.

The complete application-owned initial state commits atomically. A failure in
validation, recovery-key generation, Log Module setup, durable Audit Log
validation, or persistence leaves the Server uninitialized and safely
retryable. A non-application destination may retain an artifact such as an
empty Log Module file; its cleanup behavior belongs to its module design.

All Init failures use the Server's centralized, typed error presentation.
Interactive and bootstrap modes provide actionable, redacted output. Bootstrap
also provides a stable machine-readable category and defined nonzero exit
status. The initial categories are `already_initialized`,
`configuration_invalid`, `secret_file_unsafe`, `secret_file_unavailable`,
`storage_unavailable`, `storage_integrity_failure`, and
`initialization_failed`. Raw Rust, dependency, SQL, filesystem, and
operating-system errors never reach users or automation.

## Test Evidence

`weavelit-server-init` has direct tests for preflight-before-prompt behavior,
normalized-request validation, TOML parsing and version rejection, secret-file
safety, environment rejection, recovery-key staging and reconciliation,
redaction, atomic rollback, retry behavior, and the one-time
`AlreadyInitialized` guard.

Admin CLI process-level tests verify interactive and bootstrap command wiring,
stable normalized error output, and high-risk rejection cases. Interactive
tests use a testable input/output abstraction rather than a real terminal; the
selected no-echo terminal facility receives focused integration coverage.
Application Database integration tests verify atomic one-time persistence. A
future container bootstrap adapter has mount-based smoke tests while reusing
the same Init crate.

## Related Documents

- [Core Statements](../core-statements.md)
- [Security Model](../security-model.md)
- [Server Architecture Design](server-architecture-design.md)
- [Application Database Design](database/application-database-design.md)
- [Log Module Design](../log-modules/log-module-design.md)
- [Development Container Design](../containers/dev/development-container-design.md)
- [Production Container Design](../containers/prod/production-container-design.md)
- [Testing and Validation Policy](../testing.md)
- [Milestone 1](../plan/milestones/milestone-1.md)
