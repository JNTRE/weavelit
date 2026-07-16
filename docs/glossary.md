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
Connections, and operational state. It is selected and configured during
**[Init](#states-and-requests)**, is separate from every
**[Log Module](#applications-and-interfaces)** destination, and is not a
module. Application Database persistence uses an internal backend contract;
each supported backend is a dedicated Rust crate. The Server core owns backend
selection, common bootstrap-configuration validation, and lifecycle behavior.
Each backend validates its own connection and storage settings. Backends are
compiled into the Server package and are not runtime-installable plugins. The
MVP backend is SQLite. Application Database state and a Log Module destination
never share Weavelit-owned persistence logic or implementation crates, files,
schemas, connections, configuration, resources, lifecycle, or backup and
retention behavior. They may use the same workspace-pinned third-party
dependency, such as `rusqlite`, without sharing persistence behavior.

**Log Module** - A reusable server-side Rust library that receives pre-redacted structured **[System Logs](#applications-and-interfaces)**, **[Audit Logs](#applications-and-interfaces)**, or both and persists or delivers them to a configured destination. Log Modules are available to **[Administrators](#identities-and-access)**, disabled by default except for the module selected during **[Init](#states-and-requests)**, and configured only through server-administration functions.

**MFA Module** - A compiled-in server-side Rust library that implements one
specific **[Multifactor Authentication (MFA)](#identities-and-access)** method,
such as time-based one-time password (TOTP) or a passkey. An MFA Module owns
method-specific enrollment, verification, and protected factor-data handling;
the **[Weavelit Server](#applications-and-interfaces)** owns MFA policy,
authorization, session usability, recovery, audit records, and method
enablement. MFA Modules use maintained third-party libraries where appropriate,
are released as part of the Server package, and are not runtime-installable
plugins.

**System Log** - A structured, pre-redacted diagnostic record of Weavelit Server lifecycle events, operational state, configuration changes, authentication failures, authorization denials, dependency failures, provider failures, or internal errors. A System Log supports operation and diagnosis; it is not an Audit Log.

**Audit Log** - A structured, pre-redacted accountability record for a consequential action. It identifies the authenticated principal, **[Responsible Owner](#identities-and-access)** when applicable, action or **[Operation](#applications-and-interfaces)**, target, time, result, and correlation identifier. An Audit Log is distinct from a System Log.

**Weavelit CLI** - The separately packaged operations-only command-line
application used on a user's local system. It interacts with the
**[Weavelit Server](#applications-and-interfaces)** through the Weavelit CLI
**[Client Module](#applications-and-interfaces)**. Its first supported platform
is macOS 26 and later on Apple Silicon (`arm64`).

**Operations CLI** - Previous name for the **[Weavelit CLI](#applications-and-interfaces)**. This term is retained here only as a compatibility alias; all other documentation must use Weavelit CLI. Code or configuration may retain the previous name only where required for compatibility.

**Web UI** - The browser-based management client included with the **[Weavelit Server](#applications-and-interfaces)** and available after authentication and Init. A **[Human User](#identities-and-access)** whose **[Group](#identities-and-access)** grants the Web UI **[Client Module](#applications-and-interfaces)** can use self-service account functions and view their own Group memberships and effective access. Only an **[Administrator](#identities-and-access)** can use its administrative functions.

**Admin CLI** - The host-local server administration tool, available only to a Unix account with `sudo` authority on the Weavelit Server host.

**Client Module** - A reusable server-side Rust library that provides and maintains one client-facing connection surface to the Weavelit Server. It authenticates and translates that client's requests into the shared **[Operation](#applications-and-interfaces)** contract, while the Server remains the final authorization authority.

**Service Module** - A reusable server-side Rust library that authenticates with and communicates with one named external service through exactly one **[Service Connection](#applications-and-interfaces)** type and implements its supported Operations. Supporting the same external service through another Service Connection type requires a separately named Service Module.

**Service Connection** - A server-owned configuration through which a **[Service Module](#applications-and-interfaces)** authenticates to one external service. It specifies an authentication method and whether the resulting external identity is shared or associated with one **[Human User](#identities-and-access)**. Each Service Module supports exactly one Service Connection type; that type is unavailable until a corresponding connection is configured, and its use remains subject to all applicable caller grants. The Server receives, stores, and uses any sensitive authentication material; a Service Connection does not itself grant a caller access to the Service Module or its Operations.

**Shared API Key Service Connection** - A **[Service Connection](#applications-and-interfaces)** that uses one provider API key for all authorized callers.

**User API Key Service Connection** - A **[Service Connection](#applications-and-interfaces)** associated with one **[Human User](#identities-and-access)** that uses that user's provider API key.

**Shared OAuth Service Connection** - A **[Service Connection](#applications-and-interfaces)** that uses one OAuth authorization for all authorized callers.

**User OAuth Service Connection** - A **[Service Connection](#applications-and-interfaces)** associated with one **[Human User](#identities-and-access)** that uses an OAuth authorization for that user's external identity.

**Workflow** - A human-, agent-, or automation-owned process that uses one or more Operations, potentially across Service Modules. It is not a configurable Weavelit application object.

**Operation** - A specific named, validated, permissionable task implemented by a Service Module that the Server can authorize, audit, and execute.

## Identities and Access

**Host Administrator** - A person with `sudo` authority on the **[Weavelit Server](#applications-and-interfaces)** host who may run the **[Admin CLI](#applications-and-interfaces)**.

**Human User** - A locally or externally authenticated person.

**Group** - A collection of **[Human Users](#identities-and-access)** that grants its members access to **[Client Modules](#applications-and-interfaces)**, **[Service Modules](#applications-and-interfaces)**, named **[Operations](#applications-and-interfaces)**, and the **[Server Administration Permission](#identities-and-access)**. A Human User's effective grants are the additive union of its groups' grants; Human Users receive no direct grants.

**Server Administration Permission** - The built-in permission, granted through a **[Group](#identities-and-access)**, that allows a **[Human User](#identities-and-access)** with Web UI **[Client Module](#applications-and-interfaces)** access to administer Weavelit through the **[Web UI](#applications-and-interfaces)**. It does not itself grant Web UI Client Module access, **[Service Modules](#applications-and-interfaces)**, or named **[Operations](#applications-and-interfaces)**.

**Administrator** - A **[Human User](#identities-and-access)** whose effective group grants include the Server Administration Permission.

**Administrators Group** - The system-created **[Group](#identities-and-access)** made during Init. It grants the **[Web UI](#applications-and-interfaces)** **[Client Module](#applications-and-interfaces)** and the Server Administration Permission, but no named Operations. Its members can view logs and configure Log Modules through server-administration functions.

**Automation Identity** - A non-human principal created and managed by an **[Administrator](#identities-and-access)** with explicitly assigned named Operations for scheduled or triggered work.

**Responsible Owner** - The active **[Human User](#identities-and-access)** accountable for an **[Automation Identity](#identities-and-access)** and its configured work. Responsibility does not grant authority to change the Automation Identity's permissions or credentials.

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

**Init** - The first-time process and state in which a **[Host Administrator](#identities-and-access)** creates the **[Administrators Group](#identities-and-access)**, creates the first local **[Human User](#identities-and-access)**, adds that user to the Administrators Group, selects and configures the **[Application Database](#applications-and-interfaces)** in a host-local bootstrap step, then separately selects, configures, and activates one or more initial Log Modules and assigns configured Log Modules to System Logs and Audit Logs. The same Log Module may receive both log types, but it does not reuse Application Database resources. Init must validate both assignments, including durable Audit Log recording, before the Server starts normal operation.

**Operational Request** - A typed request for a supported **[Operation](#applications-and-interfaces)** accepted through a **[Client Module](#applications-and-interfaces)** and processed by the **[Weavelit Server](#applications-and-interfaces)**.

## Related Documents

- [Vision](vision.md)
- [Core Statements](core-statements.md)
- [Security Model](security-model.md)
