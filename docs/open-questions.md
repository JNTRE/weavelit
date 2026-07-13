# Weavelit Open Questions

This document records architecture and product decisions that remain open after
the current Vision decisions. It describes the complete intended application,
not a release roadmap. Resolved decisions belong in the Vision, Core Statements,
Glossary, or an architecture decision record rather than remaining here.

## Identity and Credentials

### 1. Local human account lifecycle

How are local human accounts invited, disabled, recovered, and deleted?

### 2. MFA method expansion

Which additional compiled-in **[MFA Modules](glossary.md#applications-and-interfaces)**
will Weavelit support after TOTP, and how can a Human User enroll, replace, or
retire multiple MFA methods without weakening an MFA requirement or creating an
account-recovery gap?

### 3. Operations CLI login and credential storage

What browser-mediated login or device-approval flow does the
**[Operations CLI](glossary.md#applications-and-interfaces)** use
for **[Local Authentication](glossary.md#identities-and-access)** and
**[External Authentication](glossary.md#identities-and-access)**? Which
operating-system credential stores are supported, how are non-secret profiles
represented, and how does `logout` remove local credentials?

### 4. Automation credential lifecycle

**[Administrators](glossary.md#identities-and-access)** create and manage local
**[Automation Identities](glossary.md#identities-and-access)**. How are their
credentials generated, displayed once, stored by a scheduler or trigger,
rotated, expired, revoked, and recovered? What default and maximum validity
periods apply, and how is **[Responsible Owner](glossary.md#identities-and-access)**
transfer or suspension handled?

## Automation and Accountability

### 5. Schedules and external triggers

Which automation sources does Weavelit support: server-owned schedules,
external webhook/event triggers, headless
**[Operations CLI](glossary.md#applications-and-interfaces)**
invocations, or all of these? How are schedules represented, enabled, paused,
retried, deduplicated, and audited?

## Authorization and Administration

### 6. Permission and group model

What additional group-granted permission types, if any, are needed beyond
access to **[Client Modules](glossary.md#applications-and-interfaces)**,
**[Service Modules](glossary.md#applications-and-interfaces)**, named
**[Operations](glossary.md#applications-and-interfaces)**, and the Server
Administration Permission? Which group-grant changes require additional
confirmation or reauthentication?

### 7. Administration boundaries

Which administrative functions are available in the
**[Web UI](glossary.md#applications-and-interfaces)** after authentication and
which remain exclusively in the host-local
**[Admin CLI](glossary.md#applications-and-interfaces)**? What recovery actions
can a **[Host Administrator](glossary.md#identities-and-access)** take, and what
**[Audit Log](glossary.md#applications-and-interfaces)** is required for each
one?

## API, Security, and Operations

### 8. HTTPS edge and public API protection

Where does TLS terminate, how are certificates renewed, which ports and source
networks are allowed, and what request-size, rate-limit, CORS, and browser-CSRF
controls apply?

### 9. API contract and compatibility policy

API routes are versioned under `/api/v1/`. What is the wire format and
compatibility policy for
**[Operational Requests](glossary.md#states-and-requests)**, results, errors,
pagination, and idempotency keys? What server and Operations CLI versions are
supported together?

### 10. Application Database and log backup, retention, and recovery

The MVP **[Application Database](glossary.md#applications-and-interfaces)** is
SQLite and is selected during Init; Weavelit does not support in-place database
migration. What compatibility window and artifact-retention policy applies to
versioned Application Database backups? How are the separate System Log and
Audit Log databases and remote Log Module destinations backed up, protected,
restored, and migrated? What configuration bounds and execution behavior apply
to their independent retention and purge policies?

### 11. Secrets and provider credential management

Which **[Service Connection](glossary.md#applications-and-interfaces)** type
does each **[Service Module](glossary.md#applications-and-interfaces)** support,
and who may establish it? How are its authentication artifacts and local
automation credentials encrypted or protected by the host, rotated, revoked,
recovered, and kept out of clients, System Logs, and Audit Logs? How are
credentials used by remote Log Modules protected, rotated, revoked, and kept out
of all log output?

## Packages and Integrations

### 12. Package, update, and container model

What versioning scheme, distribution channel, artifact-integrity or signing
mechanism, update policy, and rollback procedure apply to the Ubuntu
**[Weavelit Server](glossary.md#applications-and-interfaces)** package and the
macOS **[Operations CLI](glossary.md#applications-and-interfaces)** artifact?
Which additional Operations CLI platforms are supported after macOS 26 and later
on Apple Silicon (`arm64`)? For the post-MVP OCI-compliant production Server
image, what host administration boundary applies to Admin CLI functions other
than its defined non-interactive Init bootstrap mode, and how are those actions
authorized and audited? What persistent-volume and backup model, TLS
termination, secret injection mechanism, supported orchestrators, image
provenance, and upgrade and rollback policy apply?

### 13. Zendesk reference integration

Which Zendesk **[Service Connection](glossary.md#applications-and-interfaces)**
type is supported first, and which Zendesk identity should create or update
tickets? Which ticket fields, operations, idempotency strategy, retry behavior,
and least-privileged provider permissions define the first supported
integration?

## Web UI and Developer Quality

### 14. Web UI linting and quality-gate baseline

Which project-local linting tools and rule sets govern the TypeScript and React
source for the **[Web UI](glossary.md#applications-and-interfaces)**? How are
their versions pinned, manually run by contributors, exposed through the Server
Makefile, and enforced by a GitHub Actions quality workflow alongside the Rust
quality gate?

### 15. Web UI design system

Which design system or component library, if any, will the Web UI adopt (for
example, Fluent UI or Material UI)? What accessibility, supported-browser,
theming, maintenance, bundle-size, and long-term customization criteria must it
meet, and which visual foundations remain local to Weavelit?

## Related Documents

- [Vision](vision.md)
- [Core Statements](core-statements.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
