# Weavelit Open Questions

This document records unresolved architecture and product questions as they
arise throughout Weavelit's design and development. When a question is resolved,
record the decision in its authoritative Vision, Technical Specification,
Glossary, security, or component design document and remove it from this
register.

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

The [Technical Specification](spec.md#automation-identities) settles that
**[Administrators](glossary.md#identities-and-access)** create and manage local
**[Automation Identities](glossary.md#identities-and-access)**, credentials
support Administrator-controlled revocation and expiration, and an inactive
**[Responsible Owner](glossary.md#identities-and-access)** disables the identity
until an Administrator assigns a new active owner without changing its
Operation scopes or restoring an expired or revoked credential. How are
credentials generated, displayed once, stored by a scheduler or trigger,
rotated, recovered, and bounded by default and maximum validity periods? What
confirmation, notification, and audit behavior applies when an Administrator
reassigns the owner?

## Automation and Accountability

### 4. Schedules and external triggers

Which automation sources does Weavelit support: server-owned schedules,
external webhook/event triggers, headless
**[Weavelit CLI](glossary.md#applications-and-interfaces)**
invocations, or all of these? How are schedules represented, enabled, paused,
retried, deduplicated, and audited?

## Authorization and Administration

### 5. Permission and group model

The [Authorization Design](server/authorization/authorization-design.md#grant-model)
defines the current four group-granted permission types, and grant mutations
require the documented current-session MFA step-up. What additional
group-granted permission types, if any, are needed beyond
access to **[Client Modules](glossary.md#applications-and-interfaces)**,
**[Service Modules](glossary.md#applications-and-interfaces)**, named
**[Operations](glossary.md#applications-and-interfaces)**, and the Server
Administration Permission? Does any group-grant change require a separate
confirmation beyond that step-up?

### 6. Client Module command organization for the Weavelit CLI

The Client Module capability declaration schema, plane composition, route
organization, and shared contract composition are settled in the
[Server API Contract](server/api/api-contract-design.md). The Milestone 1 Web UI
Client Module's transport-only status capability is settled in the
[Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md).
What command organization maps the
**[Weavelit CLI](glossary.md#applications-and-interfaces)**'s
**[User Plane](glossary.md#applications-and-interfaces)** and
**[Administration Plane](glossary.md#applications-and-interfaces)** functions to
the shared API surface? How does the resulting command terminology remain
distinct from host-level deployment administration and separate network-plane
architecture?

## API, Security, and Operations

### 10. Secrets and provider credential management

The [Technical Specification](spec.md#service-modules-and-connections) and
[Security Model](security-model.md) settle that each
**[Service Module](glossary.md#applications-and-interfaces)** declares one
**[Service Connection](glossary.md#applications-and-interfaces)** type, that
setup and provider authentication remain Server-owned, and that connection
credentials are protected from clients and logs. Which connection type does
each Service Module support, who may establish it, and what rotation,
revocation, and recovery policy applies? What protection and lifecycle policy
applies to credentials used by remote Log Modules?

### 15. Direct-TLS HTTP version compatibility

The [Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md)
settles the loopback-only direct-TLS boundary and fixed response contract, but
does not state which HTTP request versions the listener supports or whether a
response version varies with the request. Should the direct-TLS listener
explicitly support HTTP/1.0, restrict requests to HTTP/1.1, or define another
bounded compatibility policy? The decision needs executable direct-TLS process
compatibility evidence and validation that no cleartext listener exists,
consistent with the [Testing and Validation Policy](testing.md).

### 16. Backup-format and Server-version compatibility window

Milestone 1 **[Restore](glossary.md#states-and-requests)** accepts only an
exact match: the backup artifact's declared outer format version and its
repeated inner `format_version` must both equal `1`, and the backup's source
**[Application Database](glossary.md#applications-and-interfaces)** backend
must equal the selected backend, as settled in the
[Server Restore Design](server/lifecycle/restore/restore-design.md#eligibility-and-workflow-choice).
What backup-format and Server-version compatibility window applies once a
second backup format version or a second Application Database backend exists?
May Restore upgrade an older backup during restoration, how is a compatibility
window declared and tested, and how does an operator-facing error distinguish
an incompatible-but-recognized artifact from a corrupt one?

## Packages and Integrations

### 11. Package, update, and container model

The [Technical Specification](spec.md#distribution-and-deployment) settles the
MVP Server `.deb` target, the macOS 26-and-newer Apple Silicon (`arm64`) CLI
target, and the requirement that the OCI production image use the same
versioned, prebuilt Server output as the package. What release versioning,
artifact-integrity or signing, update, and rollback policy applies to the
package and CLI artifact, and which additional CLI platforms are supported?

The [Production Container Design](containers/prod/production-container-design.md)
settles the sibling-image boundary and the requirement to preserve the
Server-owned state without exposing client-selected paths. What persistent-
volume and backup model, TLS termination, secret injection mechanism,
supported orchestrators, image provenance, and production upgrade and rollback
policy complete that deployment contract?

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
- [Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md)
- [Testing and Validation Policy](testing.md)
- Backup and Restore protection and Milestone 1 backup-format compatibility are
  settled in the [Server Restore Design](server/lifecycle/restore/restore-design.md)
  and [Security Model](security-model.md); the future multi-version and
  multi-backend compatibility window remains open (question 16).
