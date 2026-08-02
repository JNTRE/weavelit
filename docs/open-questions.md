# Weavelit Open Questions

This document records architecture and product decisions that remain open after
the current Vision decisions. It describes the complete intended application,
not a release roadmap. Resolved decisions belong in the Vision, Technical
Specification, Glossary, or an architecture decision record rather than
remaining here.

## Identity and Credentials

### 1. MFA method expansion

Which additional compiled-in **[MFA Modules](glossary.md#applications-and-interfaces)**
will Weavelit support after TOTP, and how can a Human User enroll, replace, or
retire multiple MFA methods without weakening an MFA requirement or creating an
unintended access-loss path?

### 2. Weavelit CLI login and credential storage

What browser-mediated login or device-approval flow does the
**[Weavelit CLI](glossary.md#applications-and-interfaces)** use
for **[Local Authentication](glossary.md#identities-and-access)** and
**[External Authentication](glossary.md#identities-and-access)**? Which
operating-system credential stores are supported, how are non-secret profiles
represented, and how does `logout` remove local credentials?

### 3. Automation credential lifecycle

**[Administrators](glossary.md#identities-and-access)** create and manage local
**[Automation Identities](glossary.md#identities-and-access)**. How are their
credentials generated, displayed once, stored by a scheduler or trigger,
rotated, expired, revoked, and recovered? What default and maximum validity
periods apply? What confirmation, notification, and audit behavior applies when
an Administrator reassigns the
**[Responsible Owner](glossary.md#identities-and-access)** of an
owner-status-disabled Automation Identity?

## Automation and Accountability

### 4. Schedules and external triggers

Which automation sources does Weavelit support: server-owned schedules,
external webhook/event triggers, headless
**[Weavelit CLI](glossary.md#applications-and-interfaces)**
invocations, or all of these? How are schedules represented, enabled, paused,
retried, deduplicated, and audited?

## Authorization and Administration

### 5. Permission and group model

What additional group-granted permission types, if any, are needed beyond
access to **[Client Modules](glossary.md#applications-and-interfaces)**,
**[Service Modules](glossary.md#applications-and-interfaces)**, named
**[Operations](glossary.md#applications-and-interfaces)**, and the Server
Administration Permission? Which group-grant changes require additional
confirmation or reauthentication?

### 6. Client Module plane and Pre-Operational Surface schema

The Milestone 1 Web UI Client Module's transport-only status capability is
settled in the [Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md).
The restricted **[Init](glossary.md#states-and-requests)** and
**[Restore](glossary.md#states-and-requests)** contracts remain exposed through
each **[Client Module](glossary.md#applications-and-interfaces)**'s
**[Pre-Operational Surface](glossary.md#applications-and-interfaces)** when it
declares the corresponding capabilities and the Server is uninitialized. The
Pre-Operational Surface is distinct from the normal authenticated
**[User Plane](glossary.md#applications-and-interfaces)** and
**[Administration Plane](glossary.md#applications-and-interfaces)**. What route
and command organization maps later normal functions to their declared plane and
access class and later lifecycle functions to their declared capability? What
declaration schema composes those planes with the later lifecycle capabilities?
How does that schema represent the Web UI with Init and Restore capabilities
plus User Plane and Administration Plane functions, and the
**[Weavelit CLI](glossary.md#applications-and-interfaces)** with User Plane and
Administration Plane functions? How does the resulting route and command
terminology remain distinct from host-level deployment administration and
separate network-plane architecture?

## API, Security, and Operations

### 8. API contract and compatibility policy

The `/api/v1/status` contract and its additive-only compatibility policy are
settled in the [Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md).
What wire format and compatibility policy apply to later
**[Operational Requests](glossary.md#states-and-requests)**, restricted
**[Init](glossary.md#states-and-requests)** and
**[Restore](glossary.md#states-and-requests)** contracts, results, errors,
pagination, idempotency keys, and Server/Weavelit CLI version compatibility?

### 9. Application Database and log backup, retention, and recovery

The MVP **[Application Database](glossary.md#applications-and-interfaces)** is
SQLite and is selected through the shared pre-operational contract before
either **[Init](glossary.md#states-and-requests)** or
**[Restore](glossary.md#states-and-requests)**; Weavelit does not support
in-place database migration. What versioned backup format, cryptographic
envelope, recovery-key format, compatibility window, and artifact-retention
policy apply? How do upload retries, protected encrypted staging and cleanup,
interrupted Restore, and crash reconciliation work? What delivery and
deduplication semantics apply when Init or Restore retries its required durable
System Log completion result after application-state commit but before sealing?
Which additional fields, if any, identify the backup format without exposing
backup contents? How are the
separate System Log and Audit Log databases and remote Log Module destinations
backed up, protected, restored, and migrated? What configuration bounds and
execution behavior apply to their independent retention and purge policies?

### 10. Secrets and provider credential management

Which **[Service Connection](glossary.md#applications-and-interfaces)** type
does each **[Service Module](glossary.md#applications-and-interfaces)** support,
and who may establish it? How are its authentication artifacts and local
automation credentials encrypted or protected by the host, rotated, revoked,
recovered, and kept out of clients, System Logs, and Audit Logs? How are
credentials used by remote Log Modules protected, rotated, revoked, and kept out
of all log output?

## Packages and Integrations

### 11. Package, update, and container model

For the MVP release, what versioning scheme, distribution channel,
artifact-integrity or signing mechanism, update policy, and rollback procedure
apply to the Ubuntu
**[Weavelit Server](glossary.md#applications-and-interfaces)** package and the
macOS **[Weavelit CLI](glossary.md#applications-and-interfaces)** artifact?
Which additional Weavelit CLI platforms are supported after macOS 26 and later
on Apple Silicon (`arm64`)?

For the post-MVP OCI-compliant production Server image, how does the build and
verification workflow prove that the image contains the same versioned,
prebuilt Server release output used to assemble the `.deb` package? How are the
Server-local Application Database deployment record, locator, and typed secret
connection values persisted and protected across container replacement while
the Server retains exclusive control of their local storage paths? What
persistent-volume and backup model, TLS termination, secret injection mechanism,
supported orchestrators, image provenance, and upgrade and rollback policy
apply?

### 12. Zendesk reference integration

Which Zendesk **[Service Connection](glossary.md#applications-and-interfaces)**
type is supported first, and which Zendesk identity should create or update
tickets? Which ticket fields, operations, idempotency strategy, retry behavior,
and least-privileged provider permissions define the first supported
integration?

## Web UI and Developer Quality

### 13. Web UI design system

Which design system or component library, if any, will the Web UI adopt (for
example, Fluent UI or Material UI)? What accessibility, supported-browser,
theming, maintenance, bundle-size, and long-term customization criteria must it
meet, and which visual foundations remain local to Weavelit?

### 14. Server release-version source

The **[Weavelit Server](glossary.md#applications-and-interfaces)** crate
manifest currently defines the Server version. When release automation and
package workflows are introduced, can they reliably derive and validate every
Server release artifact's version from that manifest, including its release tag
and platform package metadata? Revisit whether that manifest remains the
appropriate single source of truth after that validation.

## Related Documents

- [Vision](vision.md)
- [Technical Specification](spec.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
