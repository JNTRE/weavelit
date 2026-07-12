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
- **[Admin User](glossary.md#identities-and-access)** accounts
  support multifactor authentication.
- The **[Operations CLI](glossary.md#applications-and-interfaces)** never
  stores provider credentials. Its user-credential storage and login flow
  are specified separately.

## Authorization

- The server derives the caller's identity from validated credentials and makes
  the final authorization decision for every
  **[Operation](glossary.md#applications-and-interfaces)**.
- Authorization is default-deny and granted to named operations, not broadly to
  provider integrations.
- **[Groups](glossary.md#identities-and-access)** can scope a human user's
  access to Client Modules, Service Modules, and named Operations. Group
  membership does not grant, remove, or change a user's role.
- The Operations CLI is operations-only. The server does not accept Operations
  CLI credentials for administrative functions.

## Automation Accountability

- Each **[Automation Identity](glossary.md#identities-and-access)** has an
  active **[Responsible Owner](glossary.md#identities-and-access)**.
- Only an **[Admin User](glossary.md#identities-and-access)** may create or
  manage an Automation Identity, including its credentials and named
  **[Operation](glossary.md#applications-and-interfaces)** scopes.
- Automation credentials are scoped to named operations and can be revoked or
  expired by an administrator.
- Audit records identify the authenticated principal that initiated an action
  and the Responsible Owner of an Automation Identity when applicable.
