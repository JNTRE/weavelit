# Weavelit Glossary

Quick reference for the canonical terms used throughout Weavelit documentation.
Canonical terms are written as bold links on first substantive use in a document
section. Later uses in that section may be plain text.

## Applications and Interfaces

**Weavelit Server** - The Ubuntu-hosted application that owns the API, policy, audit records, provider integrations, and provider credentials.
**Weavelit Client** - The separately packaged operations-only CLI used on a user's macOS, Linux, or Windows system.
**Web UI** - The browser-based administrative client included with the Weavelit Server and available after authentication and setup.
**Admin CLI** - The host-local server administration tool, available only to a Unix account with `sudo` authority on the Weavelit Server host.
**Client Module** - A reusable library within a client application that sends supported requests to the Weavelit Server.
**Service Module** - A reusable server-side library that connects to a specific external service and implements its supported workflows and operations.
**Workflow** - A deliberately supported business capability within a Service Module that can be assigned to users or automations.
**Operation** - A named, validated action within a Workflow that the Server can authorize and execute.

## Identities and Access

**Host Administrator** - A person with `sudo` authority on the Weavelit Server host who may run the Admin CLI.
**Application Administrator** - A locally or externally authenticated user with permission to administer Weavelit through the Web UI.
**Owner** - The initial local Application Administrator created during setup with authority to manage Weavelit's administrative permissions.
**Automation Identity** - A non-human principal with a scoped credential for scheduled or triggered work.
**Responsible Owner** - The active human Application Administrator accountable for an Automation Identity and its configured work.
**Local Authentication** - Weavelit's self-contained default authentication method for human users and Automation Identities.
**External Authentication** - Optional authentication through a configured external OpenID Connect identity provider.

## States and Requests

**Initial Setup** - The state before a Host Administrator creates the first local Owner and configures the Server for normal use.
**Operational Request** - A typed request for a supported Operation sent to the Weavelit Server by a client application using a Client Module.
