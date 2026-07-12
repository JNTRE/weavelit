# Weavelit Open Questions

This document records architecture and product decisions that remain open after
the current Vision decisions. It describes the complete intended application,
not a release roadmap. Resolved decisions belong in the Vision, Core Statements,
Glossary, or an architecture decision record rather than remaining here.

## Identity and Credentials

### 1. Local human account lifecycle

How are local human accounts created, invited, disabled, recovered, and
deleted? Which multifactor methods are supported, and which are required for
**[Administrators](glossary.md#identities-and-access)**?

### 2. Operations CLI login and credential storage

What browser-mediated login or device-approval flow does the
**[Operations CLI](glossary.md#applications-and-interfaces)** use
for **[Local Authentication](glossary.md#identities-and-access)** and
**[External Authentication](glossary.md#identities-and-access)**? Which
operating-system credential stores are supported, how are non-secret profiles
represented, and how does `logout` remove local credentials?

### 3. Automation credential lifecycle

**[Administrators](glossary.md#identities-and-access)** create and manage local
**[Automation Identities](glossary.md#identities-and-access)**. How are their
credentials generated, displayed once, stored by a scheduler or trigger,
rotated, expired, revoked, and recovered? What default and maximum validity
periods apply, and how is **[Responsible Owner](glossary.md#identities-and-access)**
transfer or suspension handled?

## Automation and Accountability

### 4. Schedules and external triggers

Which automation sources does Weavelit support: server-owned schedules,
external webhook/event triggers, headless
**[Operations CLI](glossary.md#applications-and-interfaces)**
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

### 6. Administration boundaries

Which administrative functions are available in the
**[Web UI](glossary.md#applications-and-interfaces)** after authentication and
which remain exclusively in the host-local
**[Admin CLI](glossary.md#applications-and-interfaces)**? What recovery actions
can a **[Host Administrator](glossary.md#identities-and-access)** take, and what
audit record is required for each one?

## API, Security, and Operations

### 7. HTTPS edge and public API protection

Where does TLS terminate, how are certificates renewed, which ports and source
networks are allowed, and what request-size, rate-limit, CORS, and browser-CSRF
controls apply?

### 8. API contract and compatibility policy

API routes are versioned under `/api/v1/`. What is the wire format and
compatibility policy for
**[Operational Requests](glossary.md#states-and-requests)**, results, errors,
pagination, and idempotency keys? What server and Operations CLI versions are
supported together?

### 9. Durable data, audit retention, and backups

Which storage technology holds policy, audit records, idempotency state,
authentication state, schedules, and provider connection state? What data is
redacted, how long is it retained, and how are backups protected and restored?

### 10. Secrets and provider credential management

Which **[Service Connection](glossary.md#applications-and-interfaces)** type
does each **[Service Module](glossary.md#applications-and-interfaces)** support,
and who may establish it? How are its authentication artifacts and local
automation credentials encrypted or protected by the host, rotated, revoked,
recovered, and kept out of clients and audit logs?

## Packages and Integrations

### 11. Package and update model

How are the Ubuntu **[Weavelit Server](glossary.md#applications-and-interfaces)** package and macOS, Linux, and Windows
**[Operations CLI](glossary.md#applications-and-interfaces)**
packages distributed, signed, updated, and rolled back? Which client platforms
are supported?

### 12. Zendesk reference integration

Which Zendesk **[Service Connection](glossary.md#applications-and-interfaces)**
type is supported first, and which Zendesk identity should create or update
tickets? Which ticket fields, operations, idempotency strategy, retry behavior,
and least-privileged provider permissions define the first supported
integration?
