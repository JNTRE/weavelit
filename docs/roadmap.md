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
  can authenticate with Zendesk.
- [ ] The Zendesk Service Module can create a ticket.
- [ ] The Zendesk Service Module can add a comment to an existing ticket.
- [ ] The Zendesk Service Module can close a ticket.

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

### 5. Build the Operations CLI

- [ ] The **[Operations CLI](glossary.md#applications-and-interfaces)** uses
  `/api/v1/` routes on the configured HTTPS listener to submit supported
  **[Operations](glossary.md#applications-and-interfaces)**.

### MVP Boundary

The MVP boundary follows the Operations CLI milestone.

## Post-MVP

### 6. Support Automation Identities

- [ ] An **[Administrator](glossary.md#identities-and-access)** can create and
  manage an **[Automation Identity](glossary.md#identities-and-access)**,
  including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- [ ] Each Automation Identity has an active
  **[Responsible Owner](glossary.md#identities-and-access)** who is a
  **[Human User](glossary.md#identities-and-access)** but cannot change the
  Automation Identity's permissions or credentials through ownership alone.

### 7. Add External Authentication

### 8. Expand supported capabilities deliberately
