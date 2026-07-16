# Weavelit Core Statements

This document records the current product, security, and technical truths about
Weavelit. Expand or replace a statement only when a clear decision has been
made.

## Maintenance Policy

This document is an initial collection of cross-cutting product, security, and
technical commitments. As a component is implemented, move its
implementation-specific commitments to that component's canonical documentation.
Do this incrementally as implementation work makes the component's ownership
clear; do not migrate statements merely to complete a wholesale reorganization.
Keep only the cross-cutting truths here, and link to the owning documentation
when its additional context is needed.

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
- Weavelit records consequential actions as **[Audit Logs](glossary.md#applications-and-interfaces)**
  and emits **[System Logs](glossary.md#applications-and-interfaces)** for
  operational diagnosis.
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
  Audit Log recording before and after consequential operations.
- System Logs record Server lifecycle events, operational state, configuration
  changes, authentication failures, authorization denials, dependency failures,
  provider failures, and internal errors.
- Audit Logs capture the caller, operation, target, time, result, and
  correlation identifier. System Logs and Audit Logs are structured and
  pre-redacted before they reach a Log Module, excluding secrets and
  unnecessary sensitive payloads.
- **[Log Modules](glossary.md#applications-and-interfaces)** are server-side
  Rust libraries that persist or deliver System Logs, Audit Logs, or both. More
  than one enabled Log Module may be active for either log type.
- The Server stores its application state, including users, sessions, policy,
  secrets, Service Connections, and operational state, in its
  **[Application Database](glossary.md#applications-and-interfaces)**. The
  Application Database is separate from every Log Module destination. The two
  never share Weavelit-owned persistence logic or implementation crates, files,
  schemas, connections, configuration, resources, lifecycle, or backup and
  retention behavior, even when both use the same technology. They may use the
  same workspace-pinned third-party dependency, such as `rusqlite`, without
  sharing persistence behavior.
- The Server isolates Application Database persistence behind an internal
  backend contract. Each supported Application Database backend is a dedicated
  Rust crate that owns its database-driver integration, schema migrations,
  transaction behavior, connection health handling, and backend-specific
  errors, including validation of its connection and storage settings. The
  Server core owns backend selection, common bootstrap-configuration validation,
  and lifecycle behavior.
- The MVP Application Database uses the SQLite backend crate.
- The MVP default Log Module uses SQLite and stores System Logs and Audit Logs
  in a database separate from the Application Database. Selecting SQLite for
  both creates separate implementations and resources.
- **[Init](glossary.md#states-and-requests)** selects and configures the
  Application Database in a host-local bootstrap step, then separately selects,
  configures, and activates one or more initial Log Modules. Init assigns
  configured Log Modules separately to System Logs and Audit Logs; the same Log
  Module may receive both types. Init does not complete, and the Server does
  not begin normal operation, until both assignments are valid and the Audit
  Log assignment can durably record Audit Logs.
- The Application Database is not a module and cannot be enabled, disabled, or
  changed after Init. Weavelit does not support in-place migration between
  Application Database technologies. Application Database backends are
  compiled into the Server package and are not runtime-installable plugins.
- Init creates a distinct backup recovery key pair. The Server retains only
  the public recovery key, and the Host Administrator retains the private
  recovery key outside Weavelit. This key pair is separate from the Server's
  at-rest key material used to protect reversibly encrypted application data.
- An Administrator can create and download a versioned, encrypted Application
  Database backup through server-administration functions. The backup contains
  the application state required to restore operational status, including
  configuration, accounts, Groups, grants, password verifiers, protected MFA
  factor data, and Service Connection credentials. The Server encrypts each
  backup for the retained public recovery key; it never stores or redisplays
  the private recovery key.
- A Host Administrator can use the Admin CLI to import a compatible backup into
  a separately initialized Server after that Server's Application Database is
  selected and configured. Import requires the private recovery key, validates
  the backup before replacing application state, invalidates all active
  sessions, and re-encrypts restored secret material with the replacement
  Server's own at-rest key. The workflow does not support in-place migration
  between Application Database technologies.
- Post-MVP, Administrators can independently configure System Log and Audit Log
  retention and purging for each Log Module.
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
- All Rust code in Weavelit, including the Server core, separately packaged
  applications, and modules, uses the Rust 1.97 stable toolchain.
- Every implementation behavior change has the automated test evidence and
  validation required by the [Testing and Validation Policy](testing.md).
- Weavelit consists of two separately packaged applications: the
  **[Weavelit Server](glossary.md#applications-and-interfaces)** and the
  **[Weavelit CLI](glossary.md#applications-and-interfaces)**.
- The Weavelit Server owns the HTTPS API, operation catalog, authorization,
  System Logs, Audit Logs, Log Module configuration, authentication
  configuration, provider integrations, and provider credentials.
- The Weavelit Server package includes the Web UI and Admin CLI.
- The Web UI is a single-page application built with TypeScript and React.
  Its production asset bundle is built as part of the Weavelit Server package,
  installed with the Server's file structure, and is not separately installed
  or released.
- The Weavelit CLI is a peer client application installed on a user's local
  machine; its first supported platform is macOS 26 and later on Apple Silicon
  (`arm64`). It does not contain provider credentials, provider integration
  logic, or administrative functions.
- The Weavelit Server and Weavelit CLI communicate through the versioned
  application interface and can be packaged and upgraded independently within
  that interface's compatibility policy.
- The **[Admin CLI](glossary.md#applications-and-interfaces)** runs only on the
  Weavelit Server host and requires a Unix account with `sudo` authority; it is
  not a remotely callable Weavelit interface.
- The Admin CLI supports interactive **[Init](glossary.md#states-and-requests)**
  and an explicit non-interactive bootstrap mode that reads a local bootstrap
  configuration file. Both use the same Server-owned initialization logic.
  Non-interactive bootstrap runs only against uninitialized Server state, reads
  sensitive bootstrap values from local files referenced by the configuration
  file rather than environment variables, and does not log or persist those
  values or the bootstrap configuration. This non-interactive mode applies only
  to Init; other Admin CLI functions remain host-local administrative actions.
- **[Init](glossary.md#states-and-requests)**, creation of the
  **[Administrators Group](glossary.md#identities-and-access)**, creation of
  the first local **[Human User](glossary.md#identities-and-access)**, and
  assignment of that user to the Administrators Group, and selection,
  configuration, activation, and System Log and Audit Log assignment of one or
  more initial Log Modules are performed through the Admin CLI.
  External-authentication configuration is optional server administration.
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
- Administrators can view System Logs and Audit Logs in a read-only Web UI
  logging area and configure Log Modules through server-administration
  functions. Future administrative surfaces may provide equivalent access.
- The Weavelit CLI requests only supported operational tasks. The Weavelit CLI
  does not implement administrative commands, and the server does not accept
  Weavelit CLI credentials for administrative functions.
- Weavelit is an API-first application with a stable, versioned, machine-
  readable interface for explicitly supported operations.
- Weavelit exposes its application interface as an authenticated HTTPS API.
- One configurable HTTPS listener serves the Web UI browser routes and the
  authenticated API routes. API routes are versioned under `/api/v1/`.
- The Weavelit CLI uses the API routes on that listener; it does not use Web
  UI browser routes.
- Network reachability is limited by TLS, firewall and other network controls,
  and client authentication.
- Weavelit provides local human accounts and local automation credentials as
  its self-contained **[Local Authentication](glossary.md#identities-and-access)** model.
- Local **[Human User](glossary.md#identities-and-access)** accounts are
  created only through server-administration functions, including the
  **[Admin CLI](glossary.md#applications-and-interfaces)** during
  **[Init](glossary.md#states-and-requests)**. Accounts can be disabled but
  are not deleted. Weavelit provides no email-based invitation or recovery
  mechanism.
- An Administrator who can access a server-administration surface can perform
  the available local-account administration functions for any local Human
  User, including themselves, such as initiating a password reset or resetting
  an MFA enrollment. A Host Administrator can use the Admin CLI to perform the
  same local-account administration functions without an application session,
  including to clear the MFA enrollment of the sole Administrator after an MFA
  lockout.
- The Server provides each supported local MFA method through a compiled-in
  **[MFA Module](glossary.md#applications-and-interfaces)**. MFA Modules are
  released as part of the Server package, not installed as runtime plugins.
- Local Human Users authenticate with a password and may enroll in
  **[Multifactor Authentication](glossary.md#identities-and-access)**. MFA is
  optional by default, and an Administrator can require it for a local Human
  User.
- Administrators configure MFA Module enablement through server-administration
  functions. A disabled MFA Module cannot enroll or verify factors.
- Init creates the first local Human User without an enrolled MFA factor. A
  Host Administrator can use the Admin CLI to reset MFA enrollment for any
  local Human User, including themselves.
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
- The Weavelit CLI and Web UI connect through
  **[Client Modules](glossary.md#applications-and-interfaces)** that translate
  requests into the same supported operation contracts. MCP adapters will use
  separate Client Modules.
- The Weavelit CLI translates user or agent commands into typed
  **[Operational Requests](glossary.md#states-and-requests)** and returns
  machine-readable results.
- The Web UI is an API client through which permitted Human Users access
  self-service, group-scoped, and server-administration functions after init.
- Weavelit derives client identity from server-validated credentials and
  authorizes every requested operation at the gateway.
- Host-level administration is separate from Weavelit's application client
  interfaces.
- The Weavelit Server is packaged as a `.deb` application for a controlled
  Ubuntu 26.04 LTS `amd64` host, where it runs as a gateway service.
- A supported OCI-compliant production image for the Weavelit Server is a
  post-MVP deployment option. It will run a verified packaged Server artifact,
  not compile the application when the container starts.
- Weavelit implements provider integrations as focused
  **[Service Module](glossary.md#applications-and-interfaces)** libraries with
  deliberately registered operations.
- Weavelit keeps provider credentials and provider API execution on the gateway
  side of the connection.
- Weavelit will offer MCP adapters through Client Modules that use the same
  supported operation contracts.

## Related Documents

- [Vision](vision.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
- [Testing and Validation Policy](testing.md)
