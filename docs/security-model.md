# Weavelit Security Model

This document records security requirements and implementation constraints that
support the product boundaries in the [Core Statements](core-statements.md). It
is not a complete implementation design.

## Authentication

- **[Local Authentication](glossary.md#identities-and-access)** is Weavelit's
  self-contained default. **[External Authentication](glossary.md#identities-and-access)** through OpenID Connect is optional.
- Local human passwords are stored only with a modern adaptive password-hashing
  algorithm from a maintained library. Passwords are never stored in plaintext
  or reversibly encrypted.
- Local human password creation, changes, resets, storage, and verification
  occur only in Server-owned authentication logic. Client Modules and client
  applications may request those workflows, but do not persist password values
  or password verifiers, or implement separate password-hashing or verification
  behavior.
- Local **[Multifactor Authentication](glossary.md#identities-and-access)** is
  optional by default. The initial supported MFA method uses a password and a
  time-based one-time password (TOTP); a Human User who enrolls in TOTP must
  complete TOTP verification whenever they authenticate, and an Administrator
  can require MFA for a local Human User.
- The Server owns local MFA policy, TOTP enrollment, verification, reset, and
  secret storage. A Human User enrolls their own TOTP factor by confirming a
  current password and a generated TOTP code. The Server may provide the TOTP
  provisioning value only to that Human User during enrollment; it never
  returns the secret after enrollment or records TOTP secrets or codes in logs
  or audit records.
- A local Human User who is required to use MFA but has not enrolled, or whose
  enrollment has been reset, cannot obtain a usable session until completing
  TOTP enrollment. An MFA reset immediately invalidates the prior enrollment.
- An Administrator can require MFA for another local Human User and reset that
  user's MFA enrollment through the Web UI. An Administrator cannot reset
  their own MFA enrollment through the Web UI. An Administrator who has
  enrolled in MFA must complete TOTP verification for the current session to
  require MFA or reset another user's MFA enrollment.
- A **[Host Administrator](glossary.md#identities-and-access)** can use the
  **[Admin CLI](glossary.md#applications-and-interfaces)** to reset MFA
  enrollment for any local Human User, including themselves. The Server records
  MFA policy changes and resets in audit records without recording TOTP secrets
  or codes.
- **[Web UI](glossary.md#applications-and-interfaces)** browser sessions use
  secure, server-managed session handling.
- The **[Operations CLI](glossary.md#applications-and-interfaces)** never
  stores provider credentials. Its user-credential storage and login flow
  are specified separately.
- The Server receives, stores, and, where applicable, refreshes or revokes the
  sensitive authentication material for every
  **[Service Connection](glossary.md#applications-and-interfaces)**, including
  connections associated with a **[Human User](glossary.md#identities-and-access)**.
  Sensitive material may be supplied only through a declared Service Connection
  setup workflow; it is never returned to, retained by, or otherwise disclosed
  to **[Client Modules](glossary.md#applications-and-interfaces)**, client
  applications, or audit records.

## Authorization

- The server derives the caller's identity from validated credentials and makes
  the final authorization decision for every
  **[Operation](glossary.md#applications-and-interfaces)**.
- Authorization is default-deny and granted to named operations, not broadly to
  provider integrations.
- **[Groups](glossary.md#identities-and-access)** are the only source of
  **[Client Module](glossary.md#applications-and-interfaces)**,
  **[Service Module](glossary.md#applications-and-interfaces)**, named
  **[Operation](glossary.md#applications-and-interfaces)**, and
  **[Server Administration Permission](glossary.md#identities-and-access)**
  grants for **[Human Users](glossary.md#identities-and-access)**. A Human
  User's effective grants are the additive union of its groups' grants.
- Every new client-facing **[Client Module](glossary.md#applications-and-interfaces)**
  and feature must declare and enforce its required grants and one access
  class: self-service, group-scoped, or server-administration. Human User
  access is delivered only through Group membership; self-service features
  still require a Group grant to the Client Module through which they are
  accessed. A disabled account, Client Module, Service Module, or Operation
  overrides any group grant.
- Browser navigation and page visibility are usability controls only. The
  Server independently authorizes every **[Web UI](glossary.md#applications-and-interfaces)**
  request and rejects administrative requests without the Server Administration
  Permission.
- The Operations CLI is operations-only. The server does not accept Operations
  CLI credentials for administrative functions.

## Automation Accountability

- Each **[Automation Identity](glossary.md#identities-and-access)** has an
  active **[Responsible Owner](glossary.md#identities-and-access)**.
- Only an **[Administrator](glossary.md#identities-and-access)** may create or
  manage an **[Automation Identity](glossary.md#identities-and-access)**,
  including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- Automation credentials are scoped to named operations and can be revoked or
  expired by an administrator.
- Audit records identify the authenticated principal that initiated an action
  and the Responsible Owner of an Automation Identity when applicable.
