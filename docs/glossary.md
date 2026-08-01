# Weavelit Glossary

This glossary owns the canonical terms and concise definitions used throughout
Weavelit documentation. It identifies each term and its essential distinctions;
behavioral requirements remain in the [Technical Specification](spec.md) and
component designs. Canonical terms are written as bold links on first
substantive use in a document section and may be plain text thereafter.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy,
Application Database, System Logs, Audit Logs, Log Module configuration,
provider integrations, and provider credentials.

**Application Database** - The server-owned persistent database for Weavelit
application state, including users, sessions, policy, secrets, Service
Connections, and operational state. Before **[Init](#states-and-requests)** or
**[Restore](#states-and-requests)**, the shared Server contract selects and
configures a compiled-in backend from client-supplied, backend-declared values;
the Server derives any local artifact paths. It is not a module and does not
share Weavelit-owned persistence resources or behavior with a
**[Log Module](#applications-and-interfaces)** destination. The MVP backend is
SQLite.

**Log Module** - A reusable server-side Rust library that receives pre-redacted
structured **[System Logs](#applications-and-interfaces)**,
**[Audit Logs](#applications-and-interfaces)**, or both and persists or delivers
them to a configured destination. Log Modules are disabled by default, initially
activated during **[Init](#states-and-requests)** or imported during
**[Restore](#states-and-requests)**, and normally configured through an
**[Administration Plane](#applications-and-interfaces)**.

**MFA Module** - A compiled-in server-side Rust library that implements one
specific **[Multifactor Authentication (MFA)](#identities-and-access)** method,
such as time-based one-time password (TOTP) or a passkey. It owns
method-specific enrollment, verification, and protected factor data; the
**[Weavelit Server](#applications-and-interfaces)** owns policy, authorization,
session usability, recovery, auditing, and method enablement.

**System Log** - A structured, pre-redacted diagnostic record of Weavelit
Server lifecycle events, operational state, configuration changes,
authentication failures, authorization denials, dependency failures, provider
failures, or internal errors. **[Init](#states-and-requests)** and
**[Restore](#states-and-requests)** produce System Logs rather than Audit Logs
because they precede authenticated application actions.

**Audit Log** - A structured, pre-redacted accountability record for a
consequential authenticated application action. It identifies the authenticated
principal, **[Responsible Owner](#identities-and-access)** when applicable,
action or **[Operation](#applications-and-interfaces)**, target, time, result,
and correlation identifier. Init and Restore actions are not Audit Logs.

**Weavelit CLI** - The separately packaged command-line client for the
**[Weavelit Server](#applications-and-interfaces)**. It uses the Weavelit CLI
**[Client Module](#applications-and-interfaces)**, which exposes both a
**[User Plane](#applications-and-interfaces)** and an
**[Administration Plane](#applications-and-interfaces)**.

**Web UI** - The browser-based management client included with the
**[Weavelit Server](#applications-and-interfaces)**. It consumes the API surface
exposed by the Web UI **[Client Module](#applications-and-interfaces)**, including
its Init and Restore capabilities before normal operation and its
**[User Plane](#applications-and-interfaces)** and
**[Administration Plane](#applications-and-interfaces)** during normal
operation.

**Client Module** - A reusable server-side Rust library that provides one
client-facing connection surface and translates accepted requests into
Server-owned contracts. It declares a **[User Plane](#applications-and-interfaces)**,
an **[Administration Plane](#applications-and-interfaces)**, or both, and may
also declare Init or Restore capabilities on a
**[Pre-Operational Surface](#applications-and-interfaces)**. Undeclared planes
and capabilities are absent, and the Server remains the final authentication
and authorization authority.

**Pre-Operational Surface** - The restricted, unauthenticated portion of a
**[Client Module](#applications-and-interfaces)** connection surface available
only while the Server is uninitialized. It exposes only the Server-owned
**[Init](#states-and-requests)** and **[Restore](#states-and-requests)** contracts
declared by that Client Module. Lifecycle state, rather than normal principal
authorization, controls its availability; it is distinct from the
**[User Plane](#applications-and-interfaces)** and
**[Administration Plane](#applications-and-interfaces)**.

**User Plane** - The normal authenticated portion of a
**[Client Module](#applications-and-interfaces)** API surface that exposes
non-administrative functions. A User Plane function has the self-service or
group-scoped access class; the plane classifies the function, not the type of
principal that may use it. It excludes server-administration and pre-operational
functions.

**Administration Plane** - The normal authenticated portion of a
**[Client Module](#applications-and-interfaces)** API surface that exposes
server-administration functions. Each Administration Plane function has the
server-administration access class and requires both access to the Client
Module and the effective
**[Server Administration Permission](#identities-and-access)**. The
Administration Plane excludes host-level administration and pre-operational
Init and Restore capabilities.

**Service Module** - A reusable server-side Rust library that implements
supported Operations for one named external service through exactly one
**[Service Connection](#applications-and-interfaces)** type. Another Service
Connection type for the same service requires a separately named Service
Module.

**Service Connection** - A server-owned configuration and protected
authentication material through which a
**[Service Module](#applications-and-interfaces)** authenticates to one external
service. It specifies the authentication method and whether the external
identity is shared or associated with one
**[Human User](#identities-and-access)**; it does not grant caller access to the
Service Module or its Operations.

**Shared API Key Service Connection** - A
**[Service Connection](#applications-and-interfaces)** that uses one provider
API key for all authorized callers.

**User API Key Service Connection** - A
**[Service Connection](#applications-and-interfaces)** associated with one
**[Human User](#identities-and-access)** that uses that user's provider API key.

**Shared OAuth Service Connection** - A
**[Service Connection](#applications-and-interfaces)** that uses one OAuth
authorization for all authorized callers.

**User OAuth Service Connection** - A
**[Service Connection](#applications-and-interfaces)** associated with one
**[Human User](#identities-and-access)** that uses an OAuth authorization for
that user's external identity.

**Workflow** - A human-, agent-, or automation-owned process that uses one or
more Operations, potentially across Service Modules. It is not a configurable
Weavelit application object.

**Operation** - A named, validated, permissionable task implemented by a Service
Module that the Server can authorize, audit, and execute.

## Identities and Access

**Human User** - A locally or externally authenticated person represented by a
Weavelit account. The Human User is active when that account is enabled.

**Group** - A collection of **[Human Users](#identities-and-access)** and the
sole source of their access grants to
**[Client Modules](#applications-and-interfaces)**,
**[Service Modules](#applications-and-interfaces)**, named
**[Operations](#applications-and-interfaces)**, and the
**[Server Administration Permission](#identities-and-access)**. A Human User's
effective grants are the additive union of its groups' grants.

**Server Administration Permission** - The built-in permission, granted through
a **[Group](#identities-and-access)**, that allows a Human User to use the
**[Administration Plane](#applications-and-interfaces)** of a
**[Client Module](#applications-and-interfaces)** they can access. It does not
itself grant access to Client Modules,
**[Service Modules](#applications-and-interfaces)**, or named
**[Operations](#applications-and-interfaces)**.

**Administrator** - A **[Human User](#identities-and-access)** whose effective
group grants include the
**[Server Administration Permission](#identities-and-access)**.

**Administrators Group** - The system-created
**[Group](#identities-and-access)** created during
**[Init](#states-and-requests)**. It grants access to the
**[Web UI](#applications-and-interfaces)** Client Module and the Server
Administration Permission, but no named Operations.

**Automation Identity** - A non-human principal created and managed by an
**[Administrator](#identities-and-access)** with explicitly assigned named
Operations for scheduled or triggered work. Its Operation scopes are
independent of its **[Responsible Owner](#identities-and-access)**'s grants, but
it is usable only while an active Responsible Owner is assigned.

**Responsible Owner** - The **[Human User](#identities-and-access)** assigned
accountability for an **[Automation Identity](#identities-and-access)** and its
configured work. Responsibility neither requires the identity's Operation
grants nor grants authority to change its permissions or credentials. An
inactive owner disables the Automation Identity until an Administrator assigns
a new active owner.

**Local Authentication** - Weavelit's self-contained default authentication
method for Human Users and Automation Identities.

**Multifactor Authentication (MFA)** - An optional or required additional
authentication factor for a local **[Human User](#identities-and-access)**. An
enrolled Human User provides MFA whenever they authenticate; the initial method
is **[Time-Based One-Time Password (TOTP)](#identities-and-access)**.

**Time-Based One-Time Password (TOTP)** - A short authentication code generated
by an authenticator application from a shared secret and the current time.

**External Authentication** - Optional authentication through a configured
external OpenID Connect identity provider.

## States and Requests

**Init** - The pre-operational process that creates application state for an
uninitialized **[Weavelit Server](#applications-and-interfaces)** in a selected
**[Application Database](#applications-and-interfaces)**. Through an Init-capable
**[Client Module](#applications-and-interfaces)**, it establishes the
**[Administrators Group](#identities-and-access)** and first local
**[Human User](#identities-and-access)** and configures initial
**[Log Module](#applications-and-interfaces)** assignments. It is mutually
exclusive with **[Restore](#states-and-requests)** and transitions the Server to
normal operation when complete.

**Restore** - The pre-operational process that imports application state from a
compatible encrypted backup into an eligible Application Database for an
uninitialized replacement Server. Through a Restore-capable Client Module, it
accepts the backup and private recovery key and transitions the replacement
Server to normal operation when complete. It is mutually exclusive with Init,
unavailable after initialization, and does not migrate between Application
Database technologies.

**Operational Request** - A typed request for a supported
**[Operation](#applications-and-interfaces)** accepted through a
**[Client Module](#applications-and-interfaces)** and processed by the
**[Weavelit Server](#applications-and-interfaces)**.

## Related Documents

- [Vision](vision.md)
- [Technical Specification](spec.md)
- [Security Model](security-model.md)
