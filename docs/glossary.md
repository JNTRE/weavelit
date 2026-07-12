# Weavelit Glossary

Quick reference for the canonical terms used throughout Weavelit documentation.
Canonical terms are written as bold links on first substantive use in a document
section. Later uses in that section may be plain text.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy, audit records, provider integrations, and provider credentials.

**Operations CLI** - The separately packaged operations-only command-line application used on a user's macOS, Linux, or Windows system.

**Web UI** - The browser-based administrative client included with the **[Weavelit Server](#applications-and-interfaces)** and available after authentication and setup.

**Admin CLI** - The host-local server administration tool, available only to a Unix account with `sudo` authority on the Weavelit Server host.

**Client Module** - A reusable server-side Rust library that provides and maintains one client-facing connection surface to the Weavelit Server. It authenticates and translates that client's requests into the shared **[Operation](#applications-and-interfaces)** contract, while the Server remains the final authorization authority.

**Service Module** - A reusable server-side Rust library that authenticates with and communicates with one named external service and implements its supported Operations.

**Workflow** - A human-, agent-, or automation-owned process that uses one or more Operations, potentially across Service Modules. It is not a configurable Weavelit application object.

**Operation** - A specific named, validated, permissionable task implemented by a Service Module that the Server can authorize, audit, and execute.

## Identities and Access

**Host Administrator** - A person with `sudo` authority on the **[Weavelit Server](#applications-and-interfaces)** host who may run the **[Admin CLI](#applications-and-interfaces)**.

**Admin Role** - The built-in role that grants a human user permission to administer Weavelit through the **[Web UI](#applications-and-interfaces)**. It does not itself grant named **[Operations](#applications-and-interfaces)**.

**Standard Role** - The built-in role for a human user without Weavelit administrative permission. It does not itself grant named Operations.

**Admin User** - A locally or externally authenticated human user assigned the **[Admin Role](#identities-and-access)**.

**Standard User** - A locally or externally authenticated human user assigned the **[Standard Role](#identities-and-access)** who may be granted named Operations directly or through **[Groups](#identities-and-access)**.

**Group** - An administrator-defined collection of human users used to control its members' access to **[Client Modules](#applications-and-interfaces)**, **[Service Modules](#applications-and-interfaces)**, and named **[Operations](#applications-and-interfaces)**. Group membership does not grant, remove, or change a user's role.

**Automation Identity** - A non-human principal created and managed by an **[Admin User](#identities-and-access)** with explicitly assigned named Operations for scheduled or triggered work.

**Responsible Owner** - The active human **[Admin User](#identities-and-access)** or **[Standard User](#identities-and-access)** accountable for an **[Automation Identity](#identities-and-access)** and its configured work. Responsibility does not grant authority to change the Automation Identity's permissions or credentials.

**Local Authentication** - Weavelit's self-contained default authentication method for human users and Automation Identities.

**External Authentication** - Optional authentication through a configured external OpenID Connect identity provider.

## States and Requests

**Init** - The first-time process and state in which a **[Host Administrator](#identities-and-access)** creates the first local **[Admin User](#identities-and-access)** and configures the Server for normal use.

**Operational Request** - A typed request for a supported **[Operation](#applications-and-interfaces)** accepted through a **[Client Module](#applications-and-interfaces)** and processed by the **[Weavelit Server](#applications-and-interfaces)**.
