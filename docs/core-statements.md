# Weavelit Core Statements

This document records the current product, security, and technical truths about
Weavelit. Expand or replace a statement only when a clear decision has been
made.

## Weavelit Is

- Weavelit is a self-hosted gateway for AI-assisted operational workflows.
- Weavelit is a boundary between an AI agent and supported external services.
- Weavelit is designed around small, explicitly named operations.
- Weavelit keeps service credentials in its trusted operating environment, not
  in agent context or ordinary client requests.
- Weavelit treats a human steward as responsible for approving consequential
  work according to policy.
- Weavelit supports human-initiated and unattended automated work.
- Every consequential **[Operation](glossary.md#applications-and-interfaces)**
  is attributable to an authenticated principal,
  and every **[Automation Identity](glossary.md#identities-and-access)** has an
  active **[Responsible Owner](glossary.md#identities-and-access)**.
- Weavelit records the outcome of supported operations so those actions can be
  understood and audited.
- Weavelit begins with Zendesk incident follow-up tickets as its reference use
  case.

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

- Weavelit accepts requests for **[Operations](glossary.md#applications-and-interfaces)** it explicitly supports.
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
- Weavelit does not execute unknown, malformed, or unauthorized
  **[Operations](glossary.md#applications-and-interfaces)**.
- Weavelit does not add provider capabilities merely because the provider API
  makes them possible.

## How Weavelit Does It

- Weavelit presents a stable, machine-readable interface for supported
  **[Operations](glossary.md#applications-and-interfaces)**.
- Weavelit separates the agent-facing request path from the policy and
  **[Service Connection](glossary.md#applications-and-interfaces)** setup
  paths. Each **[Service Module](glossary.md#applications-and-interfaces)**
  declares one Service Connection type and its setup workflow; shared
  connections may require administrator setup, while user connections may
  require the associated
  **[Human User](glossary.md#identities-and-access)**'s authorization.
- A provider that requires another Service Connection type is represented by a
  separately named Service Module rather than an alternate connection type
  within an existing Service Module.
- Weavelit derives client identity from local authentication or a configured
  external identity provider and checks permission for every operation.
- Agent skills and client-side checks improve usability, but the gateway is
  always the final security authority.
- Weavelit uses a focused **[Service Module](glossary.md#applications-and-interfaces)** library to translate each supported operation into the required
  provider API action.
- Weavelit keeps provider-specific authentication, retries, and error handling
  inside the trusted gateway environment.
- A Service Connection determines which external identity performs an approved
  operation; it does not grant a caller access. The Server separately evaluates
  the caller's **[Group](glossary.md#identities-and-access)** grants and the
  requested Operation before selecting a compatible Service Connection.
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
- Weavelit will not accept a caller's claimed identity or permission as the
  basis for authorization.
- Weavelit will not make client-side validation or agent instructions the only
  protection for a provider action.
- Weavelit will not let the agent-facing path alter authorization policy,
  service credentials, or gateway configuration.
- The agent-facing identity cannot modify Weavelit policy, host configuration,
  operations, service connections, or secrets.
- Weavelit will not require an agent to paste, view, or manage provider
  secrets, authorization codes, refresh tokens, or browser sessions.
- Weavelit will not add a new integration without defining its operations,
  permissions, authentication model, failure behavior, and maintenance
  responsibility.

## Technical Core Truths

- Weavelit is implemented in Rust.
- Weavelit consists of two separately packaged applications: the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** and the
  **[Operations CLI](glossary.md#applications-and-interfaces)**.
- The Weavelit Server owns the HTTPS API, operation catalog, authorization,
  audit records, authentication configuration, provider integrations, and
  provider credentials.
- The Weavelit Server package includes the Web UI and Admin CLI.
- The Operations CLI is a peer client application installed on a user's local
  machine; it does not contain provider credentials, provider integration
  logic, or administrative functions.
- The Weavelit Server and Operations CLI communicate through the versioned
  application interface and can be packaged and upgraded independently within
  that interface's compatibility policy.
- The **[Admin CLI](glossary.md#applications-and-interfaces)** runs only on the
  Weavelit Server host and requires a Unix account with `sudo` authority; it is
  not a remotely callable Weavelit interface.
- **[Init](glossary.md#states-and-requests)**, creation of the
  **[Administrators Group](glossary.md#identities-and-access)**, creation of
  the first local **[Human User](glossary.md#identities-and-access)**, and
  assignment of that user to the Administrators Group are performed through the
  Admin CLI. External-authentication configuration is optional server
  administration.
- After **[Init](glossary.md#states-and-requests)**, a
  **[Human User](glossary.md#identities-and-access)** with a Group grant to the
  **[Web UI](glossary.md#applications-and-interfaces)**
  **[Client Module](glossary.md#applications-and-interfaces)** can use
  self-service account functions. The Web UI provides a read-only summary of
  the Human User's Group memberships and effective Client Module, Service
  Module, and named Operation grants, but not Service Module configuration,
  Service Connection details, provider identities, or credentials.
- Web UI administrative functions require the
  **[Server Administration Permission](glossary.md#identities-and-access)** in
  addition to Web UI Client Module access. Browser navigation is a usability
  control only: the Server independently authorizes every Web UI request and
  rejects administrative requests without that permission.
- The Operations CLI requests only supported operational tasks. The Operations
  CLI does not implement administrative commands, and the server does not
  accept Operations CLI credentials for administrative functions.
- Weavelit is an API-first application with a stable, versioned, machine-
  readable interface for explicitly supported operations.
- Weavelit exposes its application interface as an authenticated HTTPS API.
- One configurable HTTPS listener serves the Web UI browser routes and the
  authenticated API routes. API routes are versioned under `/api/v1/`.
- The Operations CLI uses the API routes on that listener; it does not use Web
  UI browser routes.
- Network reachability is limited by TLS, firewall and other network controls,
  and client authentication.
- Weavelit provides local human accounts and local automation credentials as
  its self-contained **[Local Authentication](glossary.md#identities-and-access)** model.
- **[External Authentication](glossary.md#identities-and-access)** through
  OpenID Connect providers and external workload identities is optional, not a
  deployment requirement.
- Only **[Administrators](glossary.md#identities-and-access)** create, manage,
  and assign named
  **[Operation](glossary.md#applications-and-interfaces)** scopes to Automation
  Identities. Automation credentials grant only explicitly allowed operations
  and can be revoked or expired by an administrator.
- **[Groups](glossary.md#identities-and-access)** are the only source of
  **[Client Module](glossary.md#applications-and-interfaces)**,
  **[Service Module](glossary.md#applications-and-interfaces)**, named
  **[Operation](glossary.md#applications-and-interfaces)**, and
  **[Server Administration Permission](glossary.md#identities-and-access)**
  grants for **[Human Users](glossary.md#identities-and-access)**. A Human
  User's effective grants are the additive union of its groups' grants.
- Every new client-facing Client Module and feature must declare and enforce
  its required grants and one access class: self-service, group-scoped, or
  server-administration. Human User access is delivered only through Group
  membership; self-service features still require a Group grant to the Client
  Module through which they are accessed. The Server remains default-deny when
  a feature's required access is not granted.
- The Operations CLI and Web UI connect through
  **[Client Modules](glossary.md#applications-and-interfaces)** that translate
  requests into the same supported operation contracts. MCP adapters will use
  separate Client Modules.
- The Operations CLI translates user or agent commands into typed
  **[Operational Requests](glossary.md#states-and-requests)** and returns
  machine-readable results.
- The Web UI is an API client through which permitted Human Users access
  self-service, group-scoped, and server-administration functions after init.
- Weavelit derives client identity from server-validated credentials and
  authorizes every requested operation at the gateway.
- Host-level administration is separate from Weavelit's application client
  interfaces.
- The Weavelit Server is packaged as a `.deb` application for a controlled
  Ubuntu host, where it runs as a gateway service.
- Weavelit implements provider integrations as focused
  **[Service Module](glossary.md#applications-and-interfaces)** libraries with
  deliberately registered operations.
- Weavelit keeps provider credentials and provider API execution on the gateway
  side of the connection.
- Weavelit will offer MCP adapters through Client Modules that use the same
  supported operation contracts.
