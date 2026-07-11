# Weavelit: Core Statements

## Status

Draft. This document records the current truths about Weavelit. It is
intentionally short and incomplete. Expand or replace a statement only when a
clear decision has been made.

## Weavelit Is

- Weavelit is a self-hosted gateway for AI-assisted operational workflows.
- Weavelit is a boundary between an AI agent and supported external services.
- Weavelit is designed around small, explicitly named operations.
- Weavelit keeps service credentials in its trusted operating environment, not
  in agent context or ordinary client requests.
- Weavelit treats a human steward as responsible for approving consequential
  actions.
- Weavelit records the outcome of supported operations so those actions can be
  understood and audited.
- Weavelit begins with Zendesk incident follow-up tickets as its reference
  workflow.

## Weavelit Is Not

- Weavelit is not an autonomous decision-maker.
- Weavelit is not a general remote-command system.
- Weavelit is not a general-purpose HTTP or API proxy.
- Weavelit is not a place to store provider credentials in source control.
- Weavelit is not a replacement for the systems where work is owned and
  tracked.
- Weavelit is not a browser-automation platform by default.
- Weavelit is not a marketplace for unreviewed integrations or plugins.

## Weavelit Does

- Weavelit accepts requests for operations it explicitly supports.
- Weavelit defaults to deny: an unknown identity or unsupported operation is
  not allowed.
- Weavelit grants permissions per operation, not broadly per integration.
- Weavelit validates and authorizes each requested operation before contacting
  an external service.
- A denied or invalid request must not contact the provider.
- Weavelit performs the provider-specific work required by an approved,
  supported operation.
- Weavelit returns a structured result that an AI-assisted workflow can report
  and use.
- Results include a correlation identifier; errors are structured, stable, and
  never expose secrets or raw internal traces.
- Weavelit fails safely when a request is invalid, unauthorized, duplicated,
  or cannot be completed.
- A write operation must be safe to retry or protected against creating
  duplicates.
- Weavelit can grow by adding deliberate, maintained integrations with
  appropriate automation interfaces.

## Weavelit Does Not Do

- Weavelit does not give an AI agent unrestricted shell access.
- Weavelit does not let an AI agent make arbitrary network requests to a
  provider.
- Weavelit does not expose provider secrets to an AI agent.
- Weavelit does not treat authentication as proof that a human approved an
  action.
- Weavelit does not execute unknown, malformed, or unauthorized operations.
- Weavelit does not add provider capabilities merely because the provider API
  makes them possible.

## How Weavelit Does It

- Weavelit presents a stable, machine-readable interface for supported
  operations.
- Weavelit separates the agent-facing request path from the administrator path
  that manages policy, service connections, and credentials.
- Weavelit derives client identity from a trusted authentication boundary and
  checks permission for every operation.
- Agent skills and client-side checks improve usability, but the gateway is
  always the final security authority.
- Weavelit uses a focused service module to translate each supported operation
  into the required provider API action.
- Weavelit keeps provider-specific authentication, retries, and error handling
  inside the trusted gateway environment.
- Provider authentication failure stops the requested action safely; normal
  agent operations do not initiate interactive provider login.
- Weavelit applies validation, duplicate protection where appropriate, and
  audit recording before and after consequential operations.
- Audit records capture the caller, operation, target, time, result, and
  correlation identifier, while excluding secrets and unnecessary sensitive
  payloads.
- Each integration defines its supported operations, required permissions,
  authentication model, configuration, retry and rate-limit behavior, error
  behavior, and safety tests.
- Weavelit accepts an integration only when its provider offers an appropriate,
  documented automation interface.

## How Weavelit Will Not Do It

- Weavelit will not execute client-supplied commands, scripts, URLs, HTTP
  methods, or provider payloads as a generic escape hatch.
- Weavelit will not accept a caller's claimed identity, role, or permission as
  the basis for authorization.
- Weavelit will not make client-side validation or agent instructions the only
  protection for a provider action.
- Weavelit will not let the agent-facing path alter authorization policy,
  service credentials, or gateway configuration.
- The agent-facing identity cannot modify Weavelit policy, SSH configuration,
  operations, service connections, or secrets.
- Weavelit will not require an agent to paste, view, or manage provider
  secrets, authorization codes, refresh tokens, or browser sessions.
- Weavelit will not add a new integration without defining its operations,
  permissions, authentication model, failure behavior, and maintenance owner.

## Technical Core Truths

- Weavelit is implemented in Rust.
- Weavelit provides a command-line interface for supported operations and
  separate administrative tasks.
- Weavelit provides a web user interface that administrators can use to manage
  the application.
- Weavelit uses SSH to accept connections from its clients to the self-hosted
  gateway.
- Weavelit's initial client path uses a local CLI that connects to the gateway
  through SSH; the SSH server starts a fixed Weavelit command, not a general
  shell.
- Weavelit's CLI returns machine-readable results rather than conversational
  terminal text.
- Weavelit is packaged as a `.deb` application for a controlled Ubuntu host,
  where it runs as a gateway service.
- Weavelit enforces authorization at the gateway for every requested operation.
- Weavelit implements provider integrations as focused service modules with
  deliberately registered operations.
- Weavelit keeps provider credentials and provider API execution on the gateway
  side of the connection.
- Weavelit will later offer MCP and a structured standard input/output (stdio)
  interface accessible over SSH using forced commands as additional ways to
  interact with the gateway.
