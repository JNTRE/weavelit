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
- **[Web UI](glossary.md#applications-and-interfaces)** browser sessions use
  secure, server-managed session handling.
- **[Administrators](glossary.md#identities-and-access)** support multifactor
  authentication.
- The **[Operations CLI](glossary.md#applications-and-interfaces)** never
  stores provider credentials. Its user-credential storage and login flow
  are specified separately.

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
