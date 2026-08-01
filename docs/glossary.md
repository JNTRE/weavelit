# Weavelit Glossary

Quick reference for the canonical terms used throughout Weavelit documentation.
Canonical terms are written as bold links on first substantive use in a document
section. Later uses in that section may be plain text.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy,
Application Database, System Logs, Audit Logs, Log Module configuration,
provider integrations, and provider credentials.

**Application Database** - The server-owned persistent database for Weavelit
application state, including users, sessions, policy, secrets, Service
Connections, and operational state. It is selected and configured through the
shared pre-operational Server contract before **[Init](#states-and-requests)**
or **[Restore](#states-and-requests)**, is separate from every
**[Log Module](#applications-and-interfaces)** destination, and is not a
module. Application Database persistence uses an internal backend contract;
each supported backend is a dedicated Rust crate. The Server core owns backend
composition, validation and persistence of the pre-operational selection, and
lifecycle behavior. Each backend validates its own connection and storage
settings. The Server retains the minimum protected Server-local configuration
required to reopen the selected Application Database. A separate protected
Server-local deployment record, the database locator, and pending or initialized
database state carry one matching deployment identifier so completed Init or
Restore cannot be reopened by losing or replacing only one component;
application-owned configuration remains in the database. Backends are compiled
into the Server package and are not runtime-installable plugins. The MVP backend
is SQLite.
Application Database state and a Log Module destination never share
Weavelit-owned persistence logic or implementation crates, files, schemas,
connections, configuration, resources, lifecycle, or backup and retention
behavior. They may use the same workspace-pinned third-party dependency, such
as `rusqlite`, without sharing persistence behavior.

**Log Module** - A reusable server-side Rust library that receives pre-redacted
structured **[System Logs](#applications-and-interfaces)**,
**[Audit Logs](#applications-and-interfaces)**, or both and persists or delivers
them to a configured destination. Log Modules are available to
**[Administrators](#identities-and-access)**, disabled by default except for
modules activated during Init or imported by Restore, and configured only
through an **[Administration Plane](#applications-and-interfaces)**.

**MFA Module** - A compiled-in server-side Rust library that implements one
specific **[Multifactor Authentication (MFA)](#identities-and-access)** method,
such as time-based one-time password (TOTP) or a passkey. An MFA Module owns
method-specific enrollment, verification, and protected factor-data handling;
the **[Weavelit Server](#applications-and-interfaces)** owns MFA policy,
authorization, session usability, recovery, audit records, and method
enablement. MFA Modules use maintained third-party libraries where appropriate,
are released as part of the Server package, and are not runtime-installable
plugins.

**System Log** - A structured, pre-redacted diagnostic record of Weavelit
Server lifecycle events, operational state, configuration changes,
authentication failures, authorization denials, dependency failures, provider
failures, or internal errors. Init and Restore actions and results are System
Logs because they occur before authenticated application actions are available.
A durable lifecycle-completion System Log identifies the workflow, deployment,
time, result, and correlation identifier. A System Log supports operation and
diagnosis; it is not an Audit Log.

**Audit Log** - A structured, pre-redacted accountability record for a
consequential authenticated application action. It identifies the authenticated
principal, **[Responsible Owner](#identities-and-access)** when applicable,
action or **[Operation](#applications-and-interfaces)**, target, time, result,
and correlation identifier. Init and Restore actions are not Audit Logs. An
Audit Log is distinct from a System Log.

**Weavelit CLI** - The separately packaged command-line application used on a
user's local system. It interacts with the
**[Weavelit Server](#applications-and-interfaces)** through the Weavelit CLI
**[Client Module](#applications-and-interfaces)**, whose normal API surface
includes both a **[User Plane](#applications-and-interfaces)** and an
**[Administration Plane](#applications-and-interfaces)**. Its first supported
platform is macOS 26 and later on Apple Silicon (`arm64`).

**Web UI** - The browser-based management client included with the
**[Weavelit Server](#applications-and-interfaces)**. It consumes the API surface
exposed by the Web UI **[Client Module](#applications-and-interfaces)**. While
the Server is uninitialized, it presents that module's
**[Pre-Operational Surface](#applications-and-interfaces)**, which declares both
Init and Restore capabilities. During normal operation, a
**[Human User](#identities-and-access)** whose
**[Group](#identities-and-access)** grants access to the Web UI Client Module can
use its **[User Plane](#applications-and-interfaces)** functions and view their
own Group memberships and effective access. Only an
**[Administrator](#identities-and-access)** can use its
**[Administration Plane](#applications-and-interfaces)** functions.

**Client Module** - A reusable server-side Rust library that provides and
maintains one client-facing connection surface to the Weavelit Server. During
normal operation, it exposes a **[User Plane](#applications-and-interfaces)**,
an **[Administration Plane](#applications-and-interfaces)**, or both, and
translates accepted client requests into Server-owned contracts while the
Server remains the final authentication and authorization authority. A Client
Module compiles and registers only its declared planes; an undeclared plane and
its routes, handlers, and client-facing contracts are absent. Its corresponding
client implements user experience only for those declared planes. A Client
Module may separately declare a
**[Pre-Operational Surface](#applications-and-interfaces)** with an Init
capability, a Restore capability, or both. Its corresponding client implements
pre-operational workflows only for the capabilities that surface declares.

**Pre-Operational Surface** - The restricted, unauthenticated portion of a
**[Client Module](#applications-and-interfaces)** connection surface available
only while the Server is uninitialized. It exposes only the Server-owned
**[Init](#states-and-requests)** and **[Restore](#states-and-requests)** contracts
corresponding to capabilities declared by that Client Module. Trusted lifecycle
state determines its availability rather than normal principal authorization.
It remains distinct from the **[User Plane](#applications-and-interfaces)** and
**[Administration Plane](#applications-and-interfaces)**, and it is unavailable
after the deployment is initialized.

**User Plane** - The normal authenticated portion of a
**[Client Module](#applications-and-interfaces)** API surface that exposes
non-administrative functions. Each User Plane function has either the
self-service or group-scoped access class and remains subject to its declared
grants and Server authorization. The name classifies the function, not the
principal: a **[Human User](#identities-and-access)**,
**[Administrator](#identities-and-access)**, or
**[Automation Identity](#identities-and-access)** may use a User Plane function
when authorized. The User Plane does not include server-administration
functions or pre-operational Init and Restore capabilities.

**Administration Plane** - The normal authenticated portion of a
**[Client Module](#applications-and-interfaces)** API surface that exposes
server-administration functions. Each Administration Plane function has the
server-administration access class and requires both access to the Client
Module and the effective
**[Server Administration Permission](#identities-and-access)**. The
Administration Plane does not include host-level administration or the
pre-operational Init and Restore capabilities, which cannot require an existing
Administrator.

**Service Module** - A reusable server-side Rust library that authenticates with and communicates with one named external service through exactly one **[Service Connection](#applications-and-interfaces)** type and implements its supported Operations. Supporting the same external service through another Service Connection type requires a separately named Service Module.

**Service Connection** - A server-owned configuration through which a **[Service Module](#applications-and-interfaces)** authenticates to one external service. It specifies an authentication method and whether the resulting external identity is shared or associated with one **[Human User](#identities-and-access)**. Each Service Module supports exactly one Service Connection type; that type is unavailable until a corresponding connection is configured, and its use remains subject to all applicable caller grants. The Server receives, stores, and uses any sensitive authentication material; a Service Connection does not itself grant a caller access to the Service Module or its Operations.

**Shared API Key Service Connection** - A **[Service Connection](#applications-and-interfaces)** that uses one provider API key for all authorized callers.

**User API Key Service Connection** - A **[Service Connection](#applications-and-interfaces)** associated with one **[Human User](#identities-and-access)** that uses that user's provider API key.

**Shared OAuth Service Connection** - A **[Service Connection](#applications-and-interfaces)** that uses one OAuth authorization for all authorized callers.

**User OAuth Service Connection** - A **[Service Connection](#applications-and-interfaces)** associated with one **[Human User](#identities-and-access)** that uses an OAuth authorization for that user's external identity.

**Workflow** - A human-, agent-, or automation-owned process that uses one or more Operations, potentially across Service Modules. It is not a configurable Weavelit application object.

**Operation** - A specific named, validated, permissionable task implemented by a Service Module that the Server can authorize, audit, and execute.

## Identities and Access

**Human User** - A locally or externally authenticated person represented by a
Weavelit account. A Human User is active when that account is enabled and is
inactive when that account is disabled.

**Group** - A collection of **[Human Users](#identities-and-access)** that grants its members access to **[Client Modules](#applications-and-interfaces)**, **[Service Modules](#applications-and-interfaces)**, named **[Operations](#applications-and-interfaces)**, and the **[Server Administration Permission](#identities-and-access)**. A Human User's effective grants are the additive union of its groups' grants; Human Users receive no direct grants.

**Server Administration Permission** - The built-in permission, granted through
a **[Group](#identities-and-access)**, that allows a
**[Human User](#identities-and-access)** with access to a
**[Client Module](#applications-and-interfaces)** Administration Plane to use
its server-administration functions. It does not itself grant access to a
Client Module, **[Service Modules](#applications-and-interfaces)**, or named
**[Operations](#applications-and-interfaces)**.

**Administrator** - A **[Human User](#identities-and-access)** whose effective group grants include the Server Administration Permission.

**Administrators Group** - The system-created
**[Group](#identities-and-access)** made during Init. It grants the
**[Web UI](#applications-and-interfaces)**
**[Client Module](#applications-and-interfaces)** and the Server Administration
Permission, but no named Operations. Its members can view logs and configure
Log Modules through the Web UI Client Module's Administration Plane.

**Automation Identity** - A non-human principal created and managed by an
**[Administrator](#identities-and-access)** with explicitly assigned named
Operations for scheduled or triggered work. Its Operation scopes are
independent of its Responsible Owner's effective grants, but it is usable only
while an active Responsible Owner is assigned.

**Responsible Owner** - The **[Human User](#identities-and-access)** assigned
accountability for an **[Automation Identity](#identities-and-access)** and its
configured work. Responsibility does not require the owner to possess the
Automation Identity's Operation grants and does not grant authority to change
its permissions or credentials. If the owner's account becomes inactive, the
Automation Identity is disabled until an Administrator assigns a new active
Responsible Owner.

**Local Authentication** - Weavelit's self-contained default authentication method for human users and Automation Identities.

**Multifactor Authentication (MFA)** - An optional or required additional
authentication factor for a local **[Human User](#identities-and-access)**.
For the initial supported method, the Human User confirms a time-based one-time
password (TOTP) in addition to their password. A Human User who has enrolled in
MFA must provide TOTP whenever they authenticate.

**Time-Based One-Time Password (TOTP)** - A short authentication code generated
from a shared secret and the current time by an authenticator application.

**External Authentication** - Optional authentication through a configured external OpenID Connect identity provider.

## States and Requests

**Init** - The pre-operational process by which an uninitialized **[Weavelit Server](#applications-and-interfaces)** creates new application state after the shared Server contract selects and configures the **[Application Database](#applications-and-interfaces)**. It is mutually exclusive with **[Restore](#states-and-requests)** and is exposed only through Init-capable **[Client Modules](#applications-and-interfaces)** while normal application functions are unavailable. The person completing Init creates the **[Administrators Group](#identities-and-access)** and first local **[Human User](#identities-and-access)**, adds that user to the Administrators Group, and selects, configures, and activates one or more initial Log Modules with assignments for System Logs and Audit Logs. The same Log Module may receive both log types but does not reuse Application Database resources. Init completes only after the Server validates the Application Database and both Log Module assignments, commits initialized database state, durably records the Init result through the System Log assignment, and irreversibly seals its Server-local deployment record before transitioning directly to normal operation.

**Restore** - The pre-operational process by which an uninitialized replacement **[Weavelit Server](#applications-and-interfaces)** imports existing application state from a compatible encrypted backup after the shared Server contract selects and configures an eligible **[Application Database](#applications-and-interfaces)**. It is mutually exclusive with **[Init](#states-and-requests)** and is exposed only through Restore-capable **[Client Modules](#applications-and-interfaces)** while normal application functions are unavailable. The person completing Restore supplies the backup and its private recovery key; the Server validates the backup before mutation, invalidates restored sessions, re-encrypts protected secret material with the replacement Server's at-rest key, commits the restored state under the replacement deployment identifier, durably records the Restore result through the restored System Log assignment, and irreversibly seals the deployment before transitioning directly to normal operation. Restore does not support in-place migration between Application Database technologies and becomes unavailable after the deployment is initialized.

**Operational Request** - A typed request for a supported **[Operation](#applications-and-interfaces)** accepted through a **[Client Module](#applications-and-interfaces)** and processed by the **[Weavelit Server](#applications-and-interfaces)**.

## Related Documents

- [Vision](vision.md)
- [Technical Specification](spec.md)
- [Security Model](security-model.md)
