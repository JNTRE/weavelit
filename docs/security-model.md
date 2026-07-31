# Weavelit Security Model

This document records security requirements and implementation constraints that
support the product boundaries in the [Core Statements](core-statements.md). It
is not a complete implementation design.

## Maintenance Policy

This document is an initial collection of cross-cutting security requirements
and implementation constraints. As a component is implemented, move its
implementation-specific security detail to the owning documentation:

- [Authentication Design](server/authentication/authentication-design.md)
- [Authorization Design](server/authorization/authorization-design.md)
- [Automation Identity Design](server/automation-identities/automation-identity-design.md)

Do this incrementally as implementation work makes the component's ownership
clear; do not migrate requirements merely to complete a wholesale
reorganization. Keep cross-cutting security constraints here, and link to the
owning documentation when its additional context is needed.

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
  behavior. During **[Init](glossary.md#states-and-requests)**, a client
  application may submit the first local Human User's password over HTTPS
  through an Init-capable Client Module only to that same Server-owned logic.
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
  During Init, the Server retains only the recovery public key and delivers the
  private recovery key once to the requesting client over HTTPS. The person
  completing Init records the private recovery key outside Weavelit. The
  private recovery key is never stored in the Application Database, Server
  configuration, container volume, logs, or ordinary backup artifact.
- Client applications transmit user-supplied initialization secrets and
  **[Restore](glossary.md#states-and-requests)** private recovery keys over
  HTTPS and do not log them, retain them after the request, or place them in
  URLs, browser history, or client-side persistent storage. A Restore-capable
  client reads a user-selected encrypted backup only to transmit it and does
  not create or retain another copy. Init-capable and Restore-capable Client
  Modules pass these inputs only to their corresponding Server-owned contracts
  and do not decrypt, log, or retain them. The Server never returns submitted
  secrets and persists each accepted secret only in its intended protected
  representation, such as a password verifier or encrypted credential.
- The Server-local Application Database locator contains no inline secret
  values or environment-variable interpolation. It may contain typed secret
  references only. The Server accepts a referenced secret file only when the
  opened object is a bounded regular non-symlink file without group or world
  access, and it never logs the secret, its contents, or its path. The locator
  is written atomically, protected against unauthorized reading and
  modification, and treated as integrity-sensitive Server configuration.
- The Server-local deployment record is separate from the Application Database
  locator. It contains a unique deployment identifier and the lifecycle state
  `Uninitialized`, `InitializationPending`, or `Initialized`. The record is
  written atomically and protected against unauthorized modification. No
  supported operation can reset or unseal `Initialized`;
  `InitializationPending` may return to `Uninitialized` only through an explicit
  Server-owned pre-operational recovery operation after the Application Database
  is confirmed to contain no application state. It can never return after Init
  or Restore commits application state. The locator and every pending or
  initialized Application Database state carry the same deployment identifier;
  a missing or mismatched component fails closed after the record leaves
  `Uninitialized`.
- An encrypted backup may be created through server-administration functions
  and downloaded by an Administrator. The Server encrypts it for the recovery
  public key and does not expose the private recovery key during ordinary
  backup operations. **[Restore](glossary.md#states-and-requests)** is available
  only on a genuinely uninitialized replacement Server after the shared
  pre-operational contract selects an eligible Application Database and before
  Init creates application state. It requires the encrypted backup and matching
  private recovery key through a Restore-capable Client Module. The private key
  authorizes decryption of that backup only; it is not an application identity,
  proof of host authority, or authorization for normal administration.
- The Server treats every submitted backup as untrusted even when the recovery
  key can decrypt it. Before application-state mutation, the Restore contract
  enforces bounded upload, cryptographic-work, structural, collection, string,
  execution-time, and concurrency limits, bounds any decompression the format
  supports, and validates the backup's authenticity, integrity, format version,
  compatibility, and complete contents. A rejected backup leaves the selected
  database uninitialized and produces only stable, redacted errors.
- Restore never persists the private recovery key, decrypted backup plaintext,
  or unwrapped backup keys. If temporary staging is required, the Server may
  persist only the bounded encrypted artifact in protected storage and removes
  it after success or failure. A successful Restore persists only the matching
  public recovery key, invalidates all restored sessions, re-encrypts protected
  secret material with the replacement Server's own at-rest key, atomically
  commits state bound to the replacement deployment identifier, verifies that
  the restored Audit Log assignment can durably record a Restore result without
  recovery secrets or backup contents, and seals the deployment before normal
  routes become available.

## Authorization

- **[Init](glossary.md#states-and-requests)** and
  **[Restore](glossary.md#states-and-requests)** are the sole unauthenticated
  application exceptions. While the Server is uninitialized, it exposes only
  these restricted pre-operational contracts through Client Modules that
  explicitly declare the corresponding capability and rejects normal
  application requests. Because no application identity exists yet, the
  deployer is responsible for limiting network access to these surfaces with
  TLS, firewall, and other network controls.
- The Server derives Init and Restore availability from trusted Server state,
  never from a client claim. No supported operation re-enables either contract
  after Init or Restore succeeds. Every mutating entry point independently
  validates the deployment record, selected database, deployment identifier,
  and eligible lifecycle state before reading request secrets or backup content
  or causing side effects; runtime routing is an additional control, not that
  entry point's authority. Before exposing routes, startup reconciles a matching
  initialized database with an `InitializationPending` deployment record by
  sealing the record; inability to persist that seal fails closed. When any
  retained anchor identifies an existing deployment, an unavailable, missing,
  malformed, unsafe, mismatched, or integrity-failing deployment record,
  configured Application Database, or locator must not cause the Server to
  expose Init or Restore as a fallback.
- A person with sufficient host authority can erase the deployment record,
  locator, and database together or replace the Server binary. Weavelit cannot
  distinguish complete removal of every persistent deployment anchor from a
  genuinely new installation; preventing or detecting that host-level action
  belongs to deployment access control, filesystem protection, and backup or
  monitoring policy.
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
- Except for the restricted pre-operational Init and Restore contracts, every
  new client-facing
  **[Client Module](glossary.md#applications-and-interfaces)** and feature must
  declare and enforce its required grants and one access class: self-service,
  group-scoped, or server-administration. Human User access is delivered only
  through Group membership; self-service features still require a Group grant
  to the Client Module through which they are accessed. A disabled account,
  Client Module, Service Module, or Operation overrides any group grant.
- Browser navigation and page visibility are usability controls only. The
  Server independently enforces its current lifecycle state and authorizes
  every normal **[Web UI](glossary.md#applications-and-interfaces)** request.
  During normal operation it rejects administrative requests without the
  Server Administration Permission; while uninitialized it accepts only the
  restricted Init and Restore contracts.
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
- [Server Init Design](server/init-design.md)
