# Weavelit Glossary

Quick reference for the canonical terms used throughout Weavelit documentation.
Canonical terms are written as bold links on first substantive use in a document
section. Later uses in that section may be plain text.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy, audit records, provider integrations, and provider credentials.

**Operations CLI** - The separately packaged operations-only command-line application used on a user's macOS, Linux, or Windows system.

**Web UI** - The browser-based management client included with the **[Weavelit Server](#applications-and-interfaces)** and available after authentication and Init.

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

**Server Administration Permission** - The built-in permission, granted through a **[Group](#identities-and-access)**, that allows a **[Human User](#identities-and-access)** to administer Weavelit through the **[Web UI](#applications-and-interfaces)**. It does not itself grant named Operations.

**Administrator** - A **[Human User](#identities-and-access)** whose effective group grants include the Server Administration Permission.

**Administrators Group** - The system-created **[Group](#identities-and-access)** made during Init. It grants the **[Web UI](#applications-and-interfaces)** **[Client Module](#applications-and-interfaces)** and the Server Administration Permission, but no named Operations.

**Automation Identity** - A non-human principal created and managed by an **[Administrator](#identities-and-access)** with explicitly assigned named Operations for scheduled or triggered work.

**Responsible Owner** - The active **[Human User](#identities-and-access)** accountable for an **[Automation Identity](#identities-and-access)** and its configured work. Responsibility does not grant authority to change the Automation Identity's permissions or credentials.

**Local Authentication** - Weavelit's self-contained default authentication method for human users and Automation Identities.

**External Authentication** - Optional authentication through a configured external OpenID Connect identity provider.

## States and Requests

**Init** - The first-time process and state in which a **[Host Administrator](#identities-and-access)** creates the **[Administrators Group](#identities-and-access)**, creates the first local **[Human User](#identities-and-access)**, adds that user to the Administrators Group, and configures the Server for normal use.

**Operational Request** - A typed request for a supported **[Operation](#applications-and-interfaces)** accepted through a **[Client Module](#applications-and-interfaces)** and processed by the **[Weavelit Server](#applications-and-interfaces)**.
