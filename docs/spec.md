# Weavelit Technical Specification

This document is Weavelit's highest-level technical specification. It defines
the product boundary, normative system behavior, security properties,
architecture, lifecycle, interfaces, packaging, and extension model from which
narrower design documents flow. Component documents MAY add detail within
these constraints, but they MUST NOT weaken or contradict this specification.
Documented requirements define intended behavior and do not by themselves
claim that the corresponding capabilities are already implemented.

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**,
**SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this
document are to be interpreted as described in
[RFC 2119](https://www.ietf.org/rfc/rfc2119.txt).

## Specification Maintenance

This specification MUST retain the cross-cutting product, security, and
technical requirements from which narrower documents derive. As implementation
gives a component clear ownership of implementation-specific requirements,
those requirements MUST move to that component's canonical design document and
this specification MUST link to that authority where its context remains
necessary. This transfer MUST occur incrementally with the implementation work;
maintainers MUST NOT perform a wholesale migration merely to reorganize the
documentation.

## Product Purpose And Boundary

Weavelit is a self-hosted gateway for AI-assisted operational workflows. It
forms a controlled boundary between an AI agent and supported external
services, presenting small, explicitly named
**[Operations](glossary.md#applications-and-interfaces)** instead of exposing a
general execution or networking facility. It MUST support both human-initiated
work and unattended automated work, while a human steward remains responsible
for approving consequential work according to policy. Zendesk incident
follow-up tickets are the reference use case with which Weavelit begins.

Weavelit MUST NOT act as an autonomous decision-maker, a general remote-command
system, or a general-purpose HTTP or API proxy. It MUST NOT provide unrestricted
shell access, accept client-supplied commands, scripts, URLs, HTTP methods, or
provider payloads as an escape hatch, or allow an agent to make arbitrary
provider network requests. It is not a browser-automation platform by default
and MUST NOT become a marketplace for unreviewed integrations or plugins.

Weavelit performs work in external systems but does not replace the systems in
which that work is owned and tracked. Provider capabilities MUST NOT be added
merely because a provider API makes them possible. A new integration MUST NOT
be accepted until its supported Operations, permissions, authentication model,
configuration, retry and rate-limit behavior, error behavior, safety tests, and
maintenance responsibility are defined, and the provider MUST offer an
appropriate documented automation interface.

Provider credentials MUST remain in the trusted gateway environment. They MUST
NOT appear in agent context, ordinary client requests, or source control, and
Weavelit MUST NOT require an agent to paste, view, or manage provider secrets,
authorization codes, refresh tokens, or browser sessions.

## System Composition And Ownership

Weavelit MUST be implemented in Rust, and all Rust code, including the Server
core, separately packaged applications, and modules, MUST use the Rust 1.97
stable toolchain. The product consists of two separately packaged applications:
the **[Weavelit Server](glossary.md#applications-and-interfaces)** and the
**[Weavelit CLI](glossary.md#applications-and-interfaces)**.

The Weavelit Server MUST be one deployable application process. The Server
core, Server-owned lifecycle and workflow crates for
**[Init](glossary.md#states-and-requests)** and
**[Restore](glossary.md#states-and-requests)**,
**[Application Database](glossary.md#applications-and-interfaces)** backends,
**[Client Modules](glossary.md#applications-and-interfaces)**,
**[Service Modules](glossary.md#applications-and-interfaces)**,
**[Log Modules](glossary.md#applications-and-interfaces)**, and
**[MFA Modules](glossary.md#applications-and-interfaces)** MUST be compiled into
that application and MUST NOT be independently deployed services or
runtime-installable plugins.

The Server MUST own the HTTPS API, Operation catalog, authorization,
**[System Logs](glossary.md#applications-and-interfaces)**,
**[Audit Logs](glossary.md#applications-and-interfaces)**, Log Module
configuration, authentication configuration, provider integrations, and
provider credentials. It MUST also own a centralized presentation layer that
maps typed failures to structured, stable client errors. The Server package
MUST include the **[Web UI](glossary.md#applications-and-interfaces)**.

Every implementation behavior change MUST include the automated test evidence
and validation REQUIRED by the
[Testing and Validation Policy](testing.md).

## Application Interface

Weavelit MUST be API-first and MUST expose a stable, versioned,
machine-readable application interface for explicitly supported Operations.
During normal operation, the Server MUST expose this interface as an
authenticated HTTPS API. One configurable HTTPS listener MUST serve the Web UI
application assets and the API routes exposed through Client Modules. During
normal operation, each Client Module MUST declare and expose a
**[User Plane](glossary.md#applications-and-interfaces)**, an
**[Administration Plane](glossary.md#applications-and-interfaces)**, or both.
Each Client Module MUST compile and register only the planes it declares; an
undeclared plane and its routes, request handlers, and client-facing contracts
MUST NOT be present in that Client Module. A corresponding client application
MUST implement commands, views, and workflows only for the planes declared by
its Client Module. Plane declaration determines interface capability, not
authorization: Client Modules and clients MUST NOT decide whether a principal
is authorized to invoke an exposed function, and the Server core MUST independently
authorize every request against the principal's effective grants.
API routes MUST be versioned under `/api/v1/`. The Web UI MUST perform every
application function as an API client through the API surface exposed by the
Web UI Client Module. Non-API Web UI routes MUST be limited to delivering the
application assets and supporting client-side navigation. The Weavelit CLI MUST
use the API surface exposed by the Weavelit CLI Client Module and MUST NOT use
non-API Web UI routes.

Before Init or Restore completes, the same listener MUST expose only restricted
pre-operational contracts through Client Modules that explicitly declare the
corresponding capability. Pre-operational capabilities MUST remain distinct
from the User Plane and Administration Plane, and normal application functions
MUST remain unavailable in this state. Network reachability MUST be limited
through TLS, firewall, and other deployment network controls. During normal
operation, client authentication is additionally REQUIRED. While the Server is
uninitialized, the deployer is responsible for restricting network access to
the unauthenticated Init and Restore capabilities.

Each client MUST communicate through the surface provided by its corresponding
Client Module. Client Modules MUST translate accepted client requests into the
same Server-owned Operation contracts. Client-side checks and agent skills MAY
improve usability, but the Server MUST remain the final authentication,
validation, and authorization authority.

## Operation Processing Contract

An **[Operational Request](glossary.md#states-and-requests)** MUST identify a
supported, typed Operation. The Server MUST derive the caller's identity from
locally validated credentials or a configured external identity provider; it
MUST NOT accept a caller's claimed identity or permission as an authorization
basis. Authentication alone MUST NOT be treated as proof that a human approved
an action.

For every request, the Server MUST validate the request and authorize the
authenticated principal for the specific Operation before contacting an
external provider. Authorization MUST be granted per Operation rather than
broadly per integration. Unknown identities and unsupported, unknown,
malformed, or unauthorized Operations MUST be denied, and a denied or invalid
request MUST NOT contact the provider. Client-side validation or agent
instructions MUST NOT serve as the only protection for a provider action.

After approval, a focused Service Module MUST translate the Operation into the
corresponding provider API action and perform the provider-specific work. Provider
authentication, retry behavior, and provider error handling MUST remain inside
the trusted gateway. Provider authentication failure MUST stop the request
safely, and a normal agent Operation MUST NOT initiate interactive provider
login.

The Server MUST apply input validation, duplicate protection where appropriate,
and Audit Log recording before and after consequential Operations. A write
Operation MUST either be safe to retry or be protected against duplicate side
effects. The Server MUST fail safely when a request is invalid, unauthorized,
duplicated, or cannot be completed.

Each result MUST be structured for reporting and subsequent workflow use and
MUST contain a correlation identifier. Error responses MUST be stable and MUST
NOT expose secrets, raw internal traces, or dependency-specific details. Every
consequential Operation MUST be attributable to an authenticated principal and
MUST produce the accountability records defined by this specification.

## Identity, Authentication, And Access

Weavelit MUST provide local human accounts and local automation credentials as
its self-contained **[Local Authentication](glossary.md#identities-and-access)**
model. **[External Authentication](glossary.md#identities-and-access)** through
OpenID Connect providers and external workload identities MAY be configured,
but it MUST remain OPTIONAL rather than a deployment requirement. External
Authentication configuration MUST be performed through an
**[Administration Plane](glossary.md#applications-and-interfaces)**.

### Human Users And Groups

Local **[Human User](glossary.md#identities-and-access)** accounts MUST be
created only through Administration Plane functions or the Init-capable Client
Module pre-operational capability during Init. Accounts MAY be disabled but
MUST NOT be deleted. Weavelit MUST NOT provide email-based invitation or
recovery.

**[Groups](glossary.md#identities-and-access)** MUST be the only source of
Client Module, Service Module, named Operation, and
**[Server Administration Permission](glossary.md#identities-and-access)** grants
for Human Users. A Human User's effective grants MUST be the additive union of
all of that user's Group grants. Every new client-facing Client Module and
feature MUST declare and enforce the grants it requires and exactly one access
class: self-service, group-scoped, or server-administration. Human User access
MUST be delivered only through Group membership; a self-service feature still
requires a Group grant to the Client Module through which it is accessed. The
Server MUST deny access by default whenever that access is absent. An inactive
Human User or a disabled Client Module, Service Module, or Operation MUST remain
unusable regardless of any Group grant. A
**[User Plane](glossary.md#applications-and-interfaces)** function MUST use the
self-service or group-scoped access class. An
**[Administration Plane](glossary.md#applications-and-interfaces)** function
MUST use the server-administration access class.

An **[Administrator](glossary.md#identities-and-access)** who can access a
Client Module's Administration Plane MAY perform the available local-account
administration functions for any local Human User, including themselves. These
functions include initiating a password reset or resetting an MFA enrollment
and MUST require an authenticated, usable Administrator session and the Server
Administration Permission. Weavelit MUST NOT provide a host-level, out-of-band,
or unauthenticated account-recovery interface.

### Automation Identities

The Server MUST restrict creation and management of an
**[Automation Identity](glossary.md#identities-and-access)** and assignment of
its named Operation scopes to Administrators. Automation credentials MUST grant
only explicitly allowed Operations and MUST support administrator-controlled
revocation and expiration. A
**[Responsible Owner](glossary.md#identities-and-access)**'s effective grants
MUST NOT determine or limit the Automation Identity's named Operation scopes;
responsibility MUST NOT grant the owner authority to manage the Automation
Identity.

Every Automation Identity MUST have a Responsible Owner assigned and MUST be
usable only while that owner remains an active Human User. If the Responsible
Owner becomes inactive, the Server MUST treat the Automation Identity as
disabled and MUST reject its authentication and Operation requests until an
Administrator assigns a new active Responsible Owner. Assigning a new active
Responsible Owner MUST NOT change the Automation Identity's named Operation
scopes or restore an expired or revoked credential. Every consequential action
performed by an Automation Identity MUST remain attributable to both its
authenticated identity and the Responsible Owner assigned when the action
occurs.

### Multifactor Authentication

Local Human Users MUST authenticate with a password and MAY enroll in
**[Multifactor Authentication](glossary.md#identities-and-access)**. MFA MUST be
OPTIONAL by default, and an Administrator MAY require it for a local Human User.
Each supported local MFA method MUST be provided by a compiled-in MFA Module
released as part of the Server package. MFA Modules MUST NOT be installed as
runtime plugins.

Administrators MUST be able to configure MFA Module enablement through an
Administration Plane. A disabled MFA Module MUST NOT enroll or verify factors.
Init MUST create the first local Human User without an enrolled MFA factor.

If no Administrator can authenticate, the deployment MUST remain inaccessible
through supported application interfaces. This fail-closed condition is an
accepted outcome. Deployment operators are responsible for selecting and
testing Administrator-account continuity, credential-custody, backup, and
Restore practices. Restore MAY reproduce unusable passwords or MFA enrollments
and MUST NOT claim to guarantee renewed administrative access.

## Service Modules And Connections

Weavelit MUST implement provider integrations as focused Service Module
libraries with deliberately registered Operations. A Service Module MUST
declare exactly one
**[Service Connection](glossary.md#applications-and-interfaces)** type, MUST
define the setup workflow for that type, and MUST keep provider authentication
and provider API execution on the gateway side of the connection. If a provider
requires another Service Connection type, that combination MUST be represented
by a separately named Service Module rather than an alternate connection type
within the existing module.

The agent-facing request path MUST remain separate from Service Connection
setup, authorization policy, the Operation catalog, service credentials,
secrets, and gateway or host configuration. An agent-facing identity MUST NOT
modify any of them. Shared connections MAY require Administrator setup, while a
connection associated with one Human User MAY require that user's
authorization.

A Service Connection determines the external identity that performs an
approved Operation; it MUST NOT grant caller access. Before selecting a
compatible Service Connection, the Server MUST separately evaluate the caller's
Group grants and the requested Operation.

## Logging And Accountability

The Server MUST record consequential authenticated application actions as
**[Audit Logs](glossary.md#applications-and-interfaces)** and MUST emit
**[System Logs](glossary.md#applications-and-interfaces)** for operational
diagnosis. System Logs MUST cover Server lifecycle events, operational state,
configuration changes, authentication failures, authorization denials,
dependency failures, provider failures, and internal errors. Init and Restore
actions and results MUST be System Logs and MUST NOT be attributed to an
authenticated principal or written as Audit Logs. Audit Logs MUST capture the
caller, Responsible Owner when applicable, action or Operation, target, time,
result, and correlation identifier.

After Init or Restore commits application state, the Server MUST durably record
that workflow's successful completion through the committed System Log
assignment before sealing the deployment. The record MUST include the workflow,
deployment identifier, time, result, and correlation identifier. Reconciliation
after interruption MUST retry completion logging and MUST NOT seal until the
result is durable.

System Logs and Audit Logs MUST be structured and pre-redacted before they
reach a Log Module. They MUST exclude secrets and unnecessary sensitive
payloads. Log Modules MUST be server-side Rust libraries that persist or
deliver System Logs, Audit Logs, or both. More than one enabled Log Module MAY
be active for either log type.

Administrators MUST be able to view System Logs and Audit Logs through a
read-only Web UI logging area and configure Log Modules through an
**[Administration Plane](glossary.md#applications-and-interfaces)**. Future
Client Modules MAY provide equivalent Administration Plane functions.

## Application Data And Persistence

The Server MUST store application state, including users, sessions, policy,
secrets, Service Connections, and operational state, in the Application
Database. The Application Database MUST be distinct from every Log Module
destination. Even when both use the same database technology, they MUST NOT
share Weavelit-owned persistence logic or implementation crates, files,
schemas, connections, configuration, resources, lifecycle, or backup and
retention behavior. They MAY use the same workspace-pinned third-party
dependency, such as `rusqlite`, without sharing persistence behavior.

Application Database persistence MUST be isolated behind an internal backend
contract. Every supported backend MUST be a dedicated Rust crate that owns its
database-driver integration, schema migrations, transaction behavior,
connection-health handling, backend-specific errors, and validation of its
connection and storage settings. Server core MUST own composition of available
backends, validation and persistence of the backend selected through the shared
pre-operational contract, and lifecycle behavior.

The MVP Application Database backend MUST be SQLite. The MVP default Log Module
MUST also use SQLite and MUST store System Logs and Audit Logs in a database
separate from the Application Database. Selecting SQLite for both MUST create
separate implementations and resources.

The Application Database is not a module. It MUST NOT be enabled, disabled, or
changed after deployment initialization. Weavelit MUST NOT support in-place
migration between Application Database technologies. Database backends MUST be
compiled into the Server package and MUST NOT be runtime-installable plugins.

## Pre-Operational Lifecycle

Before Init or Restore completes, the Server MUST operate in a restricted,
uninitialized mode. Init and Restore MUST be mutually exclusive paths to an
initialized state and MUST NOT require an existing Administrator. The Server
MUST expose each path only through Client Modules that explicitly declare the
matching pre-operational capability.

The normal Server runtime MUST compose `weavelit-server-lifecycle`,
`weavelit-server-init`, and `weavelit-server-restore`. The lifecycle crate MUST
own shared pre-operational state, Application Database selection,
serialization, and sealing authority. The Init crate MUST own creation of new
application state. The Restore crate MUST own backup validation, recovery-key
handling, and restoration of existing application state. These crates MUST
remain compiled into the Server, but their pre-operational functions MUST
become unavailable after sealing. Every mutating pre-operational entry point
MUST independently reject invocation after the deployment is initialized.

### Shared Database Selection

Before either workflow changes application state, a Client Module with the
applicable pre-operational capability MUST use the shared pre-operational Server
contract to select and configure the Application Database. The Server MUST
validate the selection and MUST persist in protected Server-local configuration
only the backend identifier, connection settings, and secret references needed
to reopen the selected database. It MUST then open the Application Database
without requiring a restart. All other application-owned configuration MUST be
stored in the Application Database.

Application Database selection and Init or Restore MUST occur through a Client
Module that declares the applicable pre-operational capability. Package
installation, service configuration, and future container adapters MUST supply
only the host and process settings needed to start the Server in restricted
uninitialized mode. They MUST NOT select the Application Database or create a
second application-configuration surface. Init MUST create, or Restore MUST
import, application configuration; authenticated
**[Administration Plane](glossary.md#applications-and-interfaces)** functions
MUST own subsequent mutable application settings.

### Deployment Binding And Sealing

On first startup, the Server MUST create a protected Server-local deployment
record containing a unique deployment identifier and lifecycle state. The
shared lifecycle MUST bind the Application Database locator and every pending
or initialized database state to that identifier. Before Init or Restore
commits initialized state, the record MUST enter `InitializationPending`. After
the database commit, the Server MUST irreversibly seal the record as
`Initialized` through supported application interfaces.

A supported interface MUST NOT unseal the record or reopen Init or Restore.
After sealing, loss, malformed state, or mismatch of any one retained
deployment record, locator, or database component MUST fail closed. Complete
removal of every persistent deployment anchor is host-level destruction and MAY
appear to the Server as a new installation.

### Init

Init MUST create the first local Human User and the
**[Administrators Group](glossary.md#identities-and-access)**, assign that user
to the Administrators Group, and separately select, configure, and activate one
or more initial Log Modules. The Administrators Group MUST grant Web UI Client
Module access and the Server Administration Permission, and it MUST NOT grant
named Operations.

Init MUST assign configured Log Modules separately to System Logs and Audit
Logs; the same Log Module MAY receive both types. Init MUST NOT complete, and
the Server MUST NOT begin normal operation, until both assignments are valid
and can durably record their assigned log type. After the Application Database
commit, the Server MUST durably record the Init result through the committed
System Log assignment before sealing. Successful Init MUST then transition the
running Server directly to normal operation without a restart.

Init MUST create a distinct backup recovery key pair. The Server MUST retain
only the public recovery key, and the person completing Init MUST receive the
private recovery key exactly once and retain it outside Weavelit. This pair MUST
be distinct from the Server's at-rest key material used to protect reversibly
encrypted application data.

### Backup And Restore

An Administrator MUST be able to create and download a versioned, encrypted
Application Database backup through an
**[Administration Plane](glossary.md#applications-and-interfaces)**. The backup
MUST contain the application state needed to restore operational status,
including configuration, accounts, Groups, grants, password verifiers,
protected MFA factor data, and Service Connection credentials. The Server MUST
encrypt every backup for the retained public recovery key and MUST NOT store or
redisplay the private recovery key.

A person MUST be able to use a Restore-capable Client Module to import a
compatible backup into a replacement Server after selecting and configuring its
Application Database and before Init creates new state. The Server-owned
Restore contract MUST require the private recovery key, validate the backup
before replacing application state, invalidate all active sessions, and
re-encrypt restored secret material with the replacement Server's at-rest key.
Restore MUST import application-owned settings, identities, Log Module
configuration, and Log Module assignments from the validated backup instead of
creating replacement initial state.

A successful Restore MUST commit the restored state under the replacement
deployment identifier, durably record the Restore result through the restored
System Log assignment, seal the replacement deployment, and transition the
running Server directly to normal operation without a restart. Restore MUST NOT
support in-place migration between Application Database technologies.

## Client Applications And User Experience

### Web UI

The **[Web UI](glossary.md#applications-and-interfaces)** MUST be a single-page
application built with TypeScript and React. Its production asset bundle MUST
be built as part of the Server package and installed within the Server file
structure. It MUST NOT be installed or released separately.

During normal operation, the Web UI Client Module MUST expose both a
**[User Plane](glossary.md#applications-and-interfaces)** and an
**[Administration Plane](glossary.md#applications-and-interfaces)**. It MUST
also declare both Init-capable and Restore-capable pre-operational capabilities.
Another Client Module MAY expose either workflow only when it declares the
matching capability and implements the corresponding Server-owned contract.

During normal operation, a Human User whose Group grants access to the Web UI
Client Module MUST be able to use its User Plane self-service account functions.
The Web UI MUST provide that user with a read-only summary of their Group
memberships and effective Client Module, Service Module, and named Operation
grants. It MUST NOT expose Service Module configuration, Service Connection
details, provider identities, or credentials in that summary.

Web UI Administration Plane functions MUST require the Server Administration
Permission in addition to Web UI Client Module access. Browser navigation MUST
function only as a usability control; the Server MUST independently authorize
every request and reject Administration Plane requests without that permission.

### Weavelit CLI

The Weavelit CLI MUST be a peer client application installed on a user's local
machine. Its first supported platform MUST be macOS 26 and later on Apple
Silicon (`arm64`). During normal operation, the Weavelit CLI Client Module MUST
expose both a **[User Plane](glossary.md#applications-and-interfaces)** and an
**[Administration Plane](glossary.md#applications-and-interfaces)**, and the CLI
MUST implement commands and workflows for both. The CLI MUST translate
operational commands into typed Operational Requests and return machine-readable
results. It MUST expose Administration Plane commands only through the
corresponding Client Module API surface, and the Server core MUST authorize
each request independently.

The CLI MUST NOT contain provider credentials or provider integration logic.
The Server and CLI MUST communicate through the versioned application interface
and MAY be packaged and upgraded independently within that interface's
compatibility policy.

Host-level administration MUST remain separate from all Weavelit application
client interfaces.

## Distribution And Deployment

The Server MUST be packaged as a `.deb` application for a controlled Ubuntu
26.04 LTS `amd64` host, where it runs as a gateway service. The `.deb` package
MUST be the MVP production distribution and deployment format.

## Future Capabilities

The requirements in this section are approved product direction but are not
claims of current availability.

After the MVP, the Server MUST provide a supported OCI-compliant production
image containing the same versioned, prebuilt Server release output used to
assemble the `.deb` package. The image MUST be a sibling delivery wrapper rather
than a separate Server build, and it MUST NOT compile the application when the
container starts.

Weavelit MUST offer MCP adapters through separate Client Modules that use the
same supported Operation contracts as other clients.

After the MVP, Administrators MUST be able to configure System Log and Audit Log
retention and purging independently for each Log Module.

Weavelit MAY grow through deliberate, maintained integrations that satisfy the
acceptance requirements in this specification and provide appropriate
automation interfaces.

## Related Documents

- [Vision](vision.md)
- [Security Model](security-model.md)
- [Glossary](glossary.md)
- [Server Lifecycle Design](server/lifecycle/lifecycle-design.md)
- [Server Init Design](server/lifecycle/init/init-design.md)
- [Server Restore Design](server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](testing.md)
