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
  first local **[Human User](glossary.md#identities-and-access)** without MFA
  enrollment, and add that user to the Administrators Group.
- [ ] The Administrators Group grants the Web UI
  **[Client Module](glossary.md#applications-and-interfaces)** and the
  **[Server Administration Permission](glossary.md#identities-and-access)**,
  but no named Operations.
- [ ] The **[Weavelit Server](glossary.md#applications-and-interfaces)**
  applies default-deny authorization using additive Group grants and global
  availability gates. A disabled Human User, Client Module,
  **[Service Module](glossary.md#applications-and-interfaces)**, or
  **[Operation](glossary.md#applications-and-interfaces)** is unavailable
  regardless of otherwise effective Group grants.
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
- [ ] A Host Administrator can use the Admin CLI to reset MFA enrollment for
  any local Human User, including themselves. The reset immediately invalidates
  the prior enrollment, and the Admin CLI never displays or replaces the user's
  MFA secret.
- [ ] The Server stores every local human password set or reset through Init,
  the Admin CLI, or the Web UI in accordance with the
  [Security Model](security-model.md#authentication): using a modern adaptive
  password-hashing algorithm from a maintained library, never in plaintext or
  reversibly encrypted.
- [ ] The Server evaluates each local Human User's MFA policy, enabled
  **[MFA Modules](glossary.md#applications-and-interfaces)**, and verified MFA
  factors before granting a usable session. A Human User whose account requires
  MFA cannot obtain a usable session until they verify an enabled MFA method.
- [ ] **[Init](glossary.md#states-and-requests)** generates and uses a
  self-signed TLS certificate by default.
- [ ] One configured HTTPS listener serves both the
  **[Web UI](glossary.md#applications-and-interfaces)** browser routes and
  authenticated `/api/v1/` routes.

### 2. Build the TOTP MFA Module

- [ ] The TOTP **[MFA Module](glossary.md#applications-and-interfaces)** is
  compiled into the Weavelit Server package, registered with the Server, and
  enabled by default after Init. An Administrator can enable or disable it
  through server-administration functions.
- [ ] The TOTP MFA Module uses maintained, standards-compliant TOTP libraries
  to generate and verify TOTP factors without exposing its implementation
  library directly to Client Modules or client applications.
- [ ] The TOTP MFA Module generates a unique TOTP secret and provisioning value
  for a local **[Human User](glossary.md#identities-and-access)** enrollment.
  The provisioning value is available only during that Human User's enrollment
  and is not returned after enrollment completes.
- [ ] The TOTP MFA Module activates an enrollment only after the enrolling
  Human User confirms a valid generated TOTP code, and it securely stores the
  resulting factor data in the Server's trusted environment.
- [ ] The TOTP MFA Module verifies valid TOTP codes and rejects invalid,
  expired, or replayed codes. It returns a typed verification result to the
  Server without disclosing the TOTP secret or raw implementation errors.
- [ ] Disabling the TOTP MFA Module immediately prevents new TOTP enrollment
  and verification. The Server applies the defined affected-user reporting,
  session termination, and MFA-policy behavior.

### 3. Build the Web UI Client Module

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

### 4. Build the Zendesk Service Module

- [ ] The Zendesk **[Service Module](glossary.md#applications-and-interfaces)**
  declares one supported **[Service Connection](glossary.md#applications-and-interfaces)**
  type and its setup workflow, and can use a configured connection of that
  type with Zendesk.
- [ ] A Service Connection determines the external Zendesk identity used but
  does not grant caller access; a **[Human User](glossary.md#identities-and-access)**
  must have
  **[Group](glossary.md#identities-and-access)** grants to the Zendesk Service
  Module and the named **[Operation](glossary.md#applications-and-interfaces)**.
- [ ] The Server validates and authorizes each requested Zendesk Operation,
  including applicable Client Module, Human User, Zendesk Service Module, and
  named Operation availability and required Group grants, before contacting
  Zendesk. A malformed, unsupported, unavailable, or unauthorized request sends
  no Zendesk API request.
- [ ] Unavailable or failed Zendesk Service Connection authentication stops the
  requested Operation safely and never starts an interactive provider login
  from the Operations CLI.
- [ ] The Zendesk Service Module exposes named, validated Operations to create
  tickets, add comments to existing tickets, and close tickets.
- [ ] Each Zendesk write Operation has defined retry and duplicate-protection
  behavior and is safe to retry or protected against creating duplicate tickets,
  duplicate comments, or unintended ticket state changes.
- [ ] Each supported Zendesk Operation sends the appropriate Zendesk API
  request and returns a structured success or failure result.
- [ ] Zendesk credentials remain server-owned and are never exposed in client
  results or audit records.
- [ ] Successful and failed Zendesk Operations produce the required Server
  audit records.

### 5. Build the Web UI

- [ ] The initial **[Administrator](glossary.md#identities-and-access)** can
  sign in using the local username and password established during
  **[Init](glossary.md#states-and-requests)** before enrolling in MFA.
- [ ] A **[Human User](glossary.md#identities-and-access)** with
  **[Group](glossary.md#identities-and-access)**-granted Web UI
  **[Client Module](glossary.md#applications-and-interfaces)** access can sign
  in to a self-service account area.
- [ ] **[Human Users](glossary.md#identities-and-access)** using
  **[Local Authentication](glossary.md#identities-and-access)** can change the
  password for their own account when granted Web UI Client Module access.
- [ ] A local Human User can enroll their own time-based one-time password
  (TOTP) MFA factor from the self-service account area by confirming their
  current password and a generated TOTP code. The Web UI never displays the
  TOTP secret after enrollment.
- [ ] The self-service account area shows a read-only summary of the Human
  User's **[Group](glossary.md#identities-and-access)** memberships and
  effective Client Module, Service Module, and named
  **[Operation](glossary.md#applications-and-interfaces)** grants, without
  exposing Service Module configuration, Service Connection details, provider
  identities, or credentials.
- [ ] A Human User without the
  **[Server Administration Permission](glossary.md#identities-and-access)**
  cannot see Web UI administration navigation or pages; the Server rejects a
  direct request for an administrative function.
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
- [ ] An Administrator can require MFA for another local Human User or reset
  that user's MFA enrollment through the Web UI. An Administrator cannot reset
  their own MFA enrollment through the Web UI.
- [ ] An Administrator who has enrolled in MFA must complete TOTP verification
  for the current session before requiring MFA for, or resetting MFA enrollment
  for, another local Human User.
- [ ] An Administrator can create **[Groups](glossary.md#identities-and-access)**
  and add Human Users to one or more Groups.
- [ ] An Administrator can configure a Group's grants to Client Modules,
  Service Modules, named Operations, and the
  **[Server Administration Permission](glossary.md#identities-and-access)**.
- [ ] An Administrator can configure the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** web listener
  IP address and port through the Web UI.

### 6. Build the Operations CLI Client Module

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

### 7. Build the Operations CLI

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

### 8. Support Automation Identities

- [ ] An **[Administrator](glossary.md#identities-and-access)** can create and
  manage an **[Automation Identity](glossary.md#identities-and-access)**,
  including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- [ ] Each Automation Identity has an active
  **[Responsible Owner](glossary.md#identities-and-access)** who is a
  **[Human User](glossary.md#identities-and-access)** but cannot change the
  Automation Identity's permissions or credentials through ownership alone.

### 9. Add External Authentication

### 10. Support User-Associated Service Connections

- [ ] A **[Human User](glossary.md#identities-and-access)** with Web UI
  **[Client Module](glossary.md#applications-and-interfaces)** access and a
  **[Group](glossary.md#identities-and-access)** grant to a
  **[Service Module](glossary.md#applications-and-interfaces)** can identify
  from My Access when that Service Module uses a user-associated
  **[Service Connection](glossary.md#applications-and-interfaces)** type.
- [ ] My Access provides the Service Module's declared user-associated Service
  Connection setup workflow, such as API-key entry or OAuth authorization, to
  the associated Human User. It does not provide setup for shared Service
  Connections.
- [ ] The Server receives and stores the resulting authentication material
  without returning, retaining, or otherwise disclosing it to the Web UI,
  other Human Users, or audit records.

### 11. Expand supported capabilities deliberately
