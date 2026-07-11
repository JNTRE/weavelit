# Weavelit Glossary

Quick reference for the canonical terms used throughout Weavelit documentation.
Canonical terms are written as bold links on first substantive use in a document
section. Later uses in that section may be plain text.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy, audit records, provider integrations, and provider credentials.
**Client CLI** - The separately packaged operations-only command-line application used on a user's macOS, Linux, or Windows system.
**Web UI** - The browser-based administrative client included with the Weavelit Server and available after authentication and setup.
**Admin CLI** - The host-local server administration tool, available only to a Unix account with `sudo` authority on the Weavelit Server host.
**Client Module** - A reusable server-side Rust library that provides and maintains one client-facing connection surface to the Weavelit Server. It authenticates and translates that client's requests into the shared Operation contract, while the Server remains the final authorization authority.
**Service Module** - A reusable server-side Rust library that authenticates with and communicates with one named external service and implements its supported Operations.
**Workflow** - A human-, agent-, or automation-owned process that uses one or more Operations, potentially across Service Modules. It is not a configurable Weavelit application object.
**Operation** - A specific named, validated, permissionable task implemented by a Service Module that the Server can authorize, audit, and execute.

## Identities and Access

**Host Administrator** - A person with `sudo` authority on the Weavelit Server host who may run the Admin CLI.
**Application Administrator** - A locally or externally authenticated user with permission to administer Weavelit through the Web UI.
**Automation Identity** - A non-human principal with a scoped credential for scheduled or triggered work.
**Responsible Owner** - The active human Application Administrator accountable for an Automation Identity and its configured work.
**Local Authentication** - Weavelit's self-contained default authentication method for human users and Automation Identities.
**External Authentication** - Optional authentication through a configured external OpenID Connect identity provider.

## States and Requests

**Initial Setup** - The state before a Host Administrator creates the first local Application Administrator and configures the Server for normal use.
**Operational Request** - A typed request for a supported Operation accepted through a Client Module and processed by the Weavelit Server.
