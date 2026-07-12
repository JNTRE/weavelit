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
  **[Init](glossary.md#states-and-requests)** and create the first
  local **[Admin User](glossary.md#identities-and-access)**.
- [ ] A Host Administrator can use the Admin CLI to set the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** listening IP
  address.
- [ ] A Host Administrator can start and stop the Weavelit Server process
  successfully.
- [ ] **[Init](glossary.md#states-and-requests)** generates and uses a
  self-signed TLS certificate by default.
- [ ] One configured HTTPS listener serves both the
  **[Web UI](glossary.md#applications-and-interfaces)** browser routes and
  authenticated `/api/v1/` routes.

### 2. Build the Web UI Client Module

### 3. Build the Zendesk Service Module

- [ ] The Zendesk **[Service Module](glossary.md#applications-and-interfaces)**
  can authenticate with Zendesk.
- [ ] The Zendesk Service Module can create a ticket.
- [ ] The Zendesk Service Module can add a comment to an existing ticket.
- [ ] The Zendesk Service Module can close a ticket.

### 4. Build the Web UI

- [ ] The **[Admin User](glossary.md#identities-and-access)**
  can sign in using the local username and password established during **[Init](glossary.md#states-and-requests)**.
- [ ] **[Admin Users](glossary.md#identities-and-access)** and
  **[Standard Users](glossary.md#identities-and-access)** using
  **[Local Authentication](glossary.md#identities-and-access)** can change the
  password for their own account.
- [ ] An Admin User can enable or disable a
  **[Client Module](glossary.md#applications-and-interfaces)**; its connection
  surface is unavailable while disabled.
- [ ] An Admin User can enable or disable a
  **[Service Module](glossary.md#applications-and-interfaces)**; its
  **[Operations](glossary.md#applications-and-interfaces)** are unavailable
  while disabled.
- [ ] An Admin User can enable or disable one or more
  **[Operations](glossary.md#applications-and-interfaces)**; the disabled
  Operation is unavailable to every human user and
  **[Automation Identity](glossary.md#identities-and-access)**.
- [ ] An Admin User can access account management.
- [ ] An Admin User can enable or disable human user accounts.
- [ ] An Admin User can create groups and add users to one or
  more groups.
- [ ] An Admin User can enable or disable a group's access to Client Modules,
  Service Modules, and named Operations without changing its members' roles.
- [ ] An Admin User can configure the
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

- [ ] An **[Admin User](glossary.md#identities-and-access)** can create and
  manage an **[Automation Identity](glossary.md#identities-and-access)**,
  including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- [ ] Each Automation Identity has an active
  **[Responsible Owner](glossary.md#identities-and-access)** who may be an Admin
  User or Standard User but cannot change the Automation Identity's permissions
  or credentials through ownership alone.

### 7. Add External Authentication

### 8. Expand supported capabilities deliberately
