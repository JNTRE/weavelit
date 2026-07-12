# Weavelit Roadmap

This is a non-binding capability roadmap for Weavelit. It records major
milestones in the intended product path without establishing product, security,
or technical commitments. Those commitments belong in the
[Vision](vision.md), [Core Statements](core-statements.md),
[Security Model](security-model.md), and [Glossary](glossary.md).

## Planning Guardrails

- This guide organizes the intended path without promising every listed
  milestone or capability.
- A capability becomes a product or technical commitment only when its decision
  is recorded in a canonical document or architecture decision record.
- Unresolved choices remain in [Open Questions](open-questions.md), even when
  they affect the intended path.

## MVP

### 1. Build the core server application

- [ ] A **[Host Administrator](glossary.md#identities-and-access)** can use the
  **[Admin CLI](glossary.md#applications-and-interfaces)** to complete
  **[Init](glossary.md#states-and-requests)**, create the
  **[Administrators Group](glossary.md#identities-and-access)**, create the
  first local **[Human User](glossary.md#identities-and-access)**, and add that
  user to the Administrators Group.
- [ ] The Administrators Group grants the Web UI
  **[Client Module](glossary.md#applications-and-interfaces)** and the
  **[Server Administration Permission](glossary.md#identities-and-access)**,
  but no named Operations.
- [ ] A Host Administrator can use the Admin CLI to set the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** listening IP
  address.
- [ ] A Host Administrator can start and stop the Weavelit Server process
  successfully.
- [ ] Required Server state persists across a restart, including accounts,
  Groups and their grants, Client Module and Service Module enablement,
  configuration, **[Service Connections](glossary.md#applications-and-interfaces)**,
  audit records, and active server-managed user sessions.
- [ ] A Host Administrator can use the Admin CLI to reset a local human user's
  password and require a password change at the user's next
  **[Local Authentication](glossary.md#identities-and-access)** login.
- [ ] **[Init](glossary.md#states-and-requests)** generates and uses a
  self-signed TLS certificate by default.
- [ ] One configured HTTPS listener serves both the
  **[Web UI](glossary.md#applications-and-interfaces)** browser routes and
  authenticated `/api/v1/` routes.

### 2. Build the Web UI Client Module

- [ ] The **[Web UI](glossary.md#applications-and-interfaces)**
  **[Client Module](glossary.md#applications-and-interfaces)** is registered
  with the Weavelit Server and mounts its browser-facing route namespace on the
  configured HTTPS listener.
- [ ] An **[Administrator](glossary.md#identities-and-access)** can enable or
  disable the Web UI Client Module; when disabled, its browser routes and
  sessions are unavailable.
- [ ] The Web UI Client Module uses secure, server-managed browser sessions and
  supports session termination.
- [ ] The Web UI Client Module derives the **[Human User](glossary.md#identities-and-access)**
  identity from the Server-managed session and never trusts identity, group, or
  permission claims supplied by the browser.
- [ ] A Human User must have Web UI Client Module access through a
  **[Group](glossary.md#identities-and-access)** before the module permits
  access.
- [ ] Every request entering through the Web UI Client Module is passed to the
  Server's shared authorization policy, including self-service, group-scoped,
  and server-administration access classes.
- [ ] The Web UI Client Module never exposes provider credentials, automation
  credentials, or internal error traces to the browser.

### 3. Build the Zendesk Service Module

- [ ] The Zendesk **[Service Module](glossary.md#applications-and-interfaces)**
  declares one supported **[Service Connection](glossary.md#applications-and-interfaces)**
  type and its setup workflow, and can use a configured connection of that
  type with Zendesk.
- [ ] A Service Connection determines the external Zendesk identity used but
  does not grant caller access; a **[Human User](glossary.md#identities-and-access)**
  must have
  **[Group](glossary.md#identities-and-access)** grants to the Zendesk Service
  Module and the named **[Operation](glossary.md#applications-and-interfaces)**.
- [ ] Unavailable or failed Zendesk Service Connection authentication stops the
  requested Operation safely and never starts an interactive provider login
  from the Operations CLI.
- [ ] The Zendesk Service Module exposes named, validated Operations to create
  tickets, add comments to existing tickets, and close tickets.
- [ ] Each supported Zendesk Operation sends the appropriate Zendesk API
  request and returns a structured success or failure result.
- [ ] Zendesk credentials remain server-owned and are never exposed in client
  results or audit records.
- [ ] Successful and failed Zendesk Operations produce the required Server
  audit records.

### 4. Build the Web UI

- [ ] The initial **[Administrator](glossary.md#identities-and-access)** can
  sign in using the local username and password established during
  **[Init](glossary.md#states-and-requests)**.
- [ ] **[Human Users](glossary.md#identities-and-access)** using
  **[Local Authentication](glossary.md#identities-and-access)** can change the
  password for their own account when granted Web UI Client Module access.
- [ ] An Administrator can enable or disable a
  **[Client Module](glossary.md#applications-and-interfaces)**; its connection
  surface is unavailable while disabled.
- [ ] An Administrator can enable or disable a
  **[Service Module](glossary.md#applications-and-interfaces)**; its
  **[Operations](glossary.md#applications-and-interfaces)** are unavailable
  while disabled.
- [ ] An Administrator can enable or disable one or more
  **[Operations](glossary.md#applications-and-interfaces)**; the disabled
  Operation is unavailable to every human user and
  **[Automation Identity](glossary.md#identities-and-access)**.
- [ ] An Administrator can access account management.
- [ ] An Administrator can create a local human user account; the new user must
  change its password at the first Local Authentication login.
- [ ] An Administrator can enable or disable human user accounts.
- [ ] An Administrator can reset another local human user's password and require a
  password change at the user's next Local Authentication login.
- [ ] An Administrator can create **[Groups](glossary.md#identities-and-access)**
  and add Human Users to one or more Groups.
- [ ] An Administrator can configure a Group's grants to Client Modules,
  Service Modules, named Operations, and the
  **[Server Administration Permission](glossary.md#identities-and-access)**.
- [ ] An Administrator can configure the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** web listener
  IP address and port through the Web UI.

### 5. Build the Operations CLI Client Module

- [ ] The **[Operations CLI](glossary.md#applications-and-interfaces)**
  **[Client Module](glossary.md#applications-and-interfaces)** is registered
  with the **[Weavelit Server](glossary.md#applications-and-interfaces)** and
  mounts its authenticated request namespace under `/api/v1/` on the configured
  HTTPS listener.
- [ ] An **[Administrator](glossary.md#identities-and-access)** can enable or
  disable the Operations CLI Client Module; when disabled, its API routes are
  unavailable.
- [ ] The Operations CLI Client Module authenticates the caller with
  Server-validated credentials, derives the caller identity from those
  credentials, and never trusts identity, group, or permission claims supplied
  by the Operations CLI.
- [ ] A **[Human User](glossary.md#identities-and-access)** must have
  Operations CLI Client Module access through a
  **[Group](glossary.md#identities-and-access)** before the module permits
  access.
- [ ] Every request entering through the Operations CLI Client Module is
  translated into a validated **[Operational Request](glossary.md#states-and-requests)**
  for a supported **[Operation](glossary.md#applications-and-interfaces)** and
  is passed to the Server's shared authorization policy.
- [ ] The Operations CLI Client Module permits operations-only access and does
  not accept Operations CLI credentials for administrative functions.
- [ ] The Operations CLI Client Module never exposes provider credentials,
  automation credentials, or internal error traces to the Operations CLI.

### 6. Build the Operations CLI

- [ ] A **[Human User](glossary.md#identities-and-access)** can sign in to the
  **[Operations CLI](glossary.md#applications-and-interfaces)** only when a
  **[Group](glossary.md#identities-and-access)** grants access to the
  Operations CLI **[Client Module](glossary.md#applications-and-interfaces)**.
- [ ] A Human User can sign out of the Operations CLI; subsequent requests are
  not permitted through the Operations CLI Client Module until the user signs
  in again.
- [ ] The Operations CLI uses `/api/v1/` routes on the configured HTTPS
  listener to submit supported **[Operations](glossary.md#applications-and-interfaces)**.
- [ ] An **[Administrator](glossary.md#identities-and-access)** with Group
  grants to the Operations CLI Client Module and a named Operation can use the
  Operations CLI, but its **[Server Administration Permission](glossary.md#identities-and-access)**
  does not provide Operations CLI access or administrative functions through
  the client.
- [ ] A Human User with Group grants to an enabled
  **[Service Module](glossary.md#applications-and-interfaces)** and a named
  Operation can invoke that Operation through the Operations CLI when the
  applicable configured **[Service Connection](glossary.md#applications-and-interfaces)**
  of that Service Module's one supported type is authenticated.
- [ ] A Human User with the required grants can invoke a supported Operation
  through the Operations CLI and receive the expected structured result.

### MVP Boundary

The MVP boundary follows the Operations CLI milestone.

## Post-MVP

### 7. Support Automation Identities

- [ ] An **[Administrator](glossary.md#identities-and-access)** can create and
  manage an **[Automation Identity](glossary.md#identities-and-access)**,
  including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- [ ] Each Automation Identity has an active
  **[Responsible Owner](glossary.md#identities-and-access)** who is a
  **[Human User](glossary.md#identities-and-access)** but cannot change the
  Automation Identity's permissions or credentials through ownership alone.

### 8. Add External Authentication

### 9. Expand supported capabilities deliberately
