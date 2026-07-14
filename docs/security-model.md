# Weavelit Security Model

This document records security requirements and implementation constraints that
support the product boundaries in the [Core Statements](core-statements.md). It
is not a complete implementation design.

## Maintenance Policy

This document is an initial collection of cross-cutting security requirements
and implementation constraints. As a component is implemented, move its
implementation-specific security detail to the owning specification:

- [Authentication Specification](server/authentication/spec.md)
- [Authorization Specification](server/authorization/spec.md)
- [Automation Identities Specification](server/automation-identities/spec.md)

Do this incrementally as implementation work makes the component's ownership
clear; do not migrate requirements merely to complete a wholesale
reorganization. Keep cross-cutting security constraints here, and link to the
owning specification when its additional context is needed.

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
- Local Human User accounts are created only through server-administration
  functions and may be disabled but are never deleted. Weavelit has no
  email-based invitation or recovery mechanism. An Administrator with access
  to a server-administration surface can initiate a password reset for any
  local Human User, including themselves. A Host Administrator can perform the
  same local-account administration functions with the Admin CLI without an
  application session.
- Local **[Multifactor Authentication](glossary.md#identities-and-access)** is
  optional by default. The initial supported MFA method uses a password and a
  time-based one-time password (TOTP); a Human User who enrolls in TOTP must
  complete TOTP verification whenever they authenticate, and an Administrator
  can require MFA for a local Human User.
- The Server owns local MFA policy, authorization, session usability, recovery,
  audit records, and **[MFA Module](glossary.md#applications-and-interfaces)**
  enablement. An MFA Module owns its method-specific enrollment, verification,
  and protected factor-data handling.
- The TOTP MFA Module enrolls a Human User's factor by confirming a current
  password and a generated TOTP code. It may provide the TOTP provisioning
  value only to that Human User during enrollment; it never returns the secret
  after enrollment or records TOTP secrets or codes in logs or audit records.
- A local Human User who is required to use MFA but has not enrolled, or whose
  enrollment has been reset, cannot obtain a usable session until completing
  TOTP enrollment. An MFA reset immediately invalidates the prior enrollment.
- An Administrator can disable an MFA Module through server-administration
  functions even when Human Users have active enrollments that depend on it.
  Before applying the change, the Server reports the number of affected Human
  Users. Disabling the MFA Module immediately prevents enrollment and
  verification through that method and terminates the affected Human Users'
  sessions.
- Disabling an MFA Module does not remove a Human User's MFA requirement. An
  affected Human User whose account requires MFA must enroll in an enabled MFA
  Module before obtaining a usable session. An affected Human User whose
  account does not require MFA may authenticate without MFA and can enroll in
  any enabled MFA Module.
- An Administrator with access to a server-administration surface can require
  MFA for, or reset the MFA enrollment of, any local Human User, including
  themselves. An Administrator who has enrolled in MFA must complete TOTP
  verification for the current session before requiring MFA or resetting an
  MFA enrollment.
- Resetting an MFA enrollment clears the prior factor data and, when MFA
  remains required, forces the Human User to enroll in an enabled MFA Module
  before obtaining a usable session. An Administrator who cannot authenticate
  cannot use an application administration surface to recover their own
  account.
- A **[Host Administrator](glossary.md#identities-and-access)** can use the
  **[Admin CLI](glossary.md#applications-and-interfaces)** without an
  application session to reset MFA enrollment for any local Human User,
  including the sole Administrator following an MFA lockout. The Server records
  MFA policy changes and resets in audit records without recording TOTP secrets
  or codes.
- **[Web UI](glossary.md#applications-and-interfaces)** browser sessions use
  secure, server-managed session handling.
- The **[Weavelit CLI](glossary.md#applications-and-interfaces)** never
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
- The Server protects reversibly encrypted application data with Server-local
  at-rest key material that is separate from the backup recovery key pair.
  During Init, the Server retains only the recovery public key; the Host
  Administrator records the private recovery key outside Weavelit. The private
  recovery key is never stored in the Application Database, Server
  configuration, container volume, logs, or ordinary backup artifact.
- An encrypted backup may be created through server-administration functions
  and downloaded by an Administrator. The Server encrypts it for the recovery
  public key and does not expose the private recovery key during ordinary
  backup operations. Recovery is a host-local Admin CLI action that requires
  the private recovery key. The replacement Server validates the backup,
  invalidates active sessions, and re-encrypts restored secret material with
  its own Server-local at-rest key.

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
- The Weavelit CLI is operations-only. The server does not accept Weavelit CLI
  credentials for administrative functions.

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

## Related Documents

- [Core Statements](core-statements.md)
- [Glossary](glossary.md)
