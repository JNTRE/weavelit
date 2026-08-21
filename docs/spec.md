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
MUST NOT be registered by, or reachable through, that Client Module. A
corresponding client application MUST implement commands, views, and workflows
only for the planes declared by
its Client Module. Plane declaration determines interface capability, not
authorization: Client Modules and clients MUST NOT decide whether a principal
is authorized to invoke an exposed function, and the Server core MUST independently
authorize every request against the principal's effective grants.

A module kind MAY be implemented as a shared contract crate that owns the common
schemas, handlers, validation, and results, together with per-module crates that
own only what genuinely differs. Shared implementation MUST NOT weaken
declaration: each module MUST still declare what it exposes, and the Server MUST
register only what a module declares.

API routes MUST be versioned under `/api/v1/`. The Web UI MUST perform every
application function as an API client through the API surface its Client Module
declares. Non-API Web UI routes MUST be limited to delivering the
application assets and supporting client-side navigation. The Weavelit CLI MUST
likewise use only the API surface its Client Module declares, and MUST NOT use
non-API Web UI routes. Client Modules MAY declare the same shared API surface;
where they do, the Server MUST serve one implementation so their behavior cannot
diverge.

Before Init or Restore completes, the same listener MUST expose only applicable
**[Pre-Operational Surfaces](glossary.md#applications-and-interfaces)** provided
by Client Modules. Each Pre-Operational Surface MUST expose only the restricted
status, Init, and Restore contracts corresponding to capabilities explicitly
declared by its Client Module. Pre-Operational Surfaces MUST remain distinct
from the User Plane and Administration Plane, and normal application functions
MUST remain unavailable in this state. The Web UI Client Module declares the
status and Application Database selection capabilities; their exact contracts
are defined by the
[Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md)
and the
[Web UI Pre-Operational Database Selection Surface](client-modules/web-ui/pre-operational-database-selection-design.md).
Network reachability MUST be limited through TLS, firewall, and other deployment
network controls. During normal operation, client authentication is additionally
REQUIRED. While the Server is uninitialized, the deployer is responsible for
restricting network access to unauthenticated pre-operational capabilities.

Each client MUST communicate through the surface provided by its corresponding
Client Module. Client Modules MUST translate each accepted client request into
its owning Server contract. Client-side checks and agent skills MAY
improve usability, but the Server MUST remain the final authentication,
validation, and authorization authority.

## HTTPS Listener And Pre-Operational Exposure

The **[Weavelit Server](glossary.md#applications-and-interfaces)** MUST
terminate TLS directly. It MUST bind only one TLS-only HTTPS listener at
the address and port explicitly supplied through trusted host configuration.
It MUST NOT provide a cleartext HTTP listener, redirect, fallback, or an
alternative TLS-termination mode. The trusted host configuration MUST provide
the filesystem paths to the PEM-encoded certificate and matching private key.
The host is responsible for certificate issuance and renewal; on every startup,
the Server MUST validate the configured listener and TLS material and fail
closed when either is missing, invalid, unreadable, mismatched, or otherwise
unusable.

The Server MUST expose **[Init](glossary.md#states-and-requests)** and
**[Restore](glossary.md#states-and-requests)** only through the declared
**[Pre-Operational Surface](glossary.md#applications-and-interfaces)** on that
listener. Trusted host configuration and deployment network controls MUST NOT
create another Init, Restore, application-configuration, or unauthenticated
administrative path.

Every Pre-Operational Surface MUST have an explicit source-network exposure
control and enforced bounds for request size, request rate, concurrent work,
parsing, decompression, cryptographic work, and execution time. Browser-
accessible routes MUST additionally apply explicit origin controls, restricted
cross-origin resource sharing (CORS), and cross-site request forgery (CSRF)
protections appropriate to their browser interaction. The
[Security Model](security-model.md) defines the corresponding security
profile; these requirements do not select an API wire format or compatibility
policy.

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
requires a Group grant to the Client Module through which it is accessed. When
Client Modules share an API surface, the Server MUST determine the Client Module
through which a request is made from the authenticated session established for
that Client Module. The
Server MUST deny access by default whenever that access is absent. An inactive
Human User or a disabled Client Module, Service Module, or Operation MUST remain
unusable regardless of any Group grant. A
**[User Plane](glossary.md#applications-and-interfaces)** function MUST use the
self-service or group-scoped access class. An
**[Administration Plane](glossary.md#applications-and-interfaces)** function
MUST use the server-administration access class.

Every Group MUST have an independent durable nonzero random 128-bit public
identifier. A client-facing interface MUST use only its canonical unpadded
Base64url representation as the Group target and MUST NOT expose or derive it
from an Application Database state identifier, Audit Reference Identifier,
name, membership, grant, or generation. Compatible Restore input MAY omit the
field only under an existing version's documented legacy rule, in which case
Restore MUST generate a fresh independent value.

An Administrator MAY create an empty Group and update its unique name and
nullable description. Group deletion MUST require current-session MFA step-up
for the GrantMutation family and MUST succeed only while the Group has no
memberships and no direct grants. It MUST NOT implicitly remove memberships or
grants, disclose association counts through its rejection, or bypass the
effective-last-Administrator protections on grant mutations.

An **[Administrator](glossary.md#identities-and-access)** who can access a
Client Module's Administration Plane MAY perform the available local-account
administration functions for any local Human User, including themselves. These
functions include initiating a password reset or resetting an MFA enrollment
and MUST require an authenticated, usable Administrator session and the Server
Administration Permission. Weavelit MUST NOT provide a host-level, out-of-band,
or unauthenticated account-recovery interface.

#### Password Reset And MFA Reset (Independent Operations)

An Administrator MAY initiate a **password reset** for any local Human User,
including themselves. Password reset is a Server-owned operation that generates
a temporary password and requires the user to change their password at the next
sign-in. The Server-generated temporary password is not recoverable after the
originating successful account-create or password-reset workflow, but an
authorized Administrator MAY receive it once in that originating workflow.
Password reset does NOT affect MFA enrollment; users retain their MFA factors
unless an Administrator separately resets their MFA.

An Administrator MAY initiate an **MFA reset** for any local Human User,
including themselves, to clear and re-enroll an MFA factor. MFA reset does NOT
affect the user's password and is independent from password reset.

Password reset and MFA reset MUST produce separate Audit Log events; one
operation MUST NOT be represented as the other.

An Administrator CANNOT remove the **Server Administration Permission** from an
account if doing so would leave no active accounts with that permission.
Attempting to do so returns a stable error: "Cannot remove the last Server
Administration Permission grant." Account disabling is permitted separately
from grant removal.

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
compatible Service Connection, the Server MUST authorize the requested Operation
according to the authenticated principal type. For a Human User, it MUST
evaluate the applicable effective Group grants. For an Automation Identity, it
MUST evaluate the explicitly assigned named Operation scopes. Service Connection
selection MUST NOT expand either authorization result.

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
A correlated Audit record set for a consequential action MUST include its final
result. A pre-commit Attempt record MAY omit the result, provided a correlated
Completion or Correction record records the authoritative outcome. An Attempt
MUST NOT be the sole accountability evidence, and this exception MUST NOT
permit an unbounded or unredacted record.

After Init or Restore commits application state, the Server MUST receive a
durable acknowledgement for that workflow's successful completion through the
committed System Log assignment before sealing the deployment. A durable
acknowledgement means that the destination completed its configured supported
storage interface's commit path during the same valid Server process run. The
record MUST include the workflow, deployment identifier, time, result, and
correlation identifier. Normal operation MUST NOT begin until every required
workflow obligation has that acknowledgement and the deployment is sealed
during the same valid workflow run. If interruption prevents completion logging
or sealing, the Server MUST apply the fail-closed lifecycle-interruption
boundary and MUST NOT retry the obligation after restart.

System Logs and Audit Logs MUST be structured and pre-redacted before they
reach a Log Module. They MUST exclude secrets and unnecessary sensitive
payloads. Log Modules MUST be server-side Rust libraries that persist or
deliver System Logs, Audit Logs, or both. More than one enabled Log Module MAY
be active for either log type.

Before creating the complete typed record delivered to a Log Module, the Server
MUST enforce UTF-8 byte limits: 64 bytes for the correlation identifier; 128
bytes for a System Log classification and 4 KiB for System Log detail; and 256
bytes for an Audit Log principal, 128 bytes for an Audit Log action, 1 KiB for
an Audit Log target, and 4 KiB for Audit Log detail. The correlation identifier
and all body fields together MUST NOT exceed 8 KiB. Empty or oversized input
MUST be rejected before complete-record construction. The Server and Log
Modules MUST NOT truncate, hash, retain raw source payloads, or create a
replacement record for rejected input. A workflow that requires logging MUST
fail when it cannot construct a valid bounded record. Rejection errors MUST be
stable and payload-free, and every destination, including future Log Modules,
MUST receive only complete bounded records.

**Consequential operations** (those that modify application state or external
systems) MUST fail with a stable error if the required Audit Log destination is
unavailable and cannot accept the record. **Non-consequential operations** (such
as read-only queries or internal-only tasks) MAY succeed even if the Audit Log
destination is temporarily unavailable; in such cases, the Server MUST record
the failure in System Logs for operator visibility.

An initialized deployment does not crash or exit if an Audit Log destination
becomes unavailable after the Server has begun normal operation. Requests for
consequential operations fail with a stable error, but the Server remains
operational for other requests. Operators MUST monitor System Logs for
`dependency.audit-log-unavailable` events and restore destination connectivity
to resume consequential operations.

An ordinary Audit destination configuration or assignment change MUST retain
every old binding identity, version, and resolvable destination handle while a
pending terminal obligation references it. Exact replay MUST continue only to
that retained binding. If repair proves the exact oldest valid active
obligation's destination permanently unavailable, an Administrator MAY
supersede it only after fresh password reauthentication for the exact current
session, fresh TOTP verification when enrolled, explicit confirmation, and
successful replacement Audit preflight. Supersession MUST append a distinct
Audit action and fixed disposition, atomically establish the replacement
assignment and its terminal recovery obligation, expose degraded Audit
completeness, and retain the immutable original for exact late delivery. Before
the append, storage MUST match the exact Server Audit-validated original
identity and immutable projection bytes and its retained binding; matching an
identifier alone is insufficient. Supersession MUST NOT be represented as a
Correction, acknowledgement, or delivery proof. Restore, System Logs, and
delivery to the replacement MUST NOT substitute for exact delivery of the
original to its retained binding.

Administrators MUST be able to view System Logs and Audit Logs through a
read-only Web UI logging area and configure Log Modules through an
**[Administration Plane](glossary.md#applications-and-interfaces)**. Client
Modules MAY provide equivalent Administration Plane functions.

Retention and purge are destination-module-owned policies. An Administrator
MAY select a retention or purge policy only where the destination declares it
relevant; the Server MUST NOT provide a Server-wide automatic purge or an
arbitrary global retention default, and a destination MAY declare retention
unsupported. System Logs MAY be purged only under a configured
destination-owned policy. Audit Logs MUST NOT be automatically purged; any
Audit Log retention or deletion capability requires a future explicit,
authorized decision that includes a hold policy. Future Administration Plane
policy, purge-start, failure, completion, and status functions MUST authorize
the Administrator, require the applicable confirmation, and create Audit Logs
for policy changes and purge start, failure, and completion. These requirements
do not add purge behavior to the current unselected SQLite Log Module catalog
scaffold.

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

The Application Database backend MUST be SQLite. The default Log Module MUST
also use SQLite and MUST store System Logs and Audit Logs in a database separate
from the Application Database. Selecting SQLite for both MUST create separate
implementations and resources.

Every Log Module destination MUST own its protection, snapshot or backup,
migration, compatibility, recovery, and retirement behavior. Application
Database backups and Restore MUST include only non-secret Log Module
configuration and assignments; they MUST NOT include destination data or
authentication or connection credentials. A restored remote destination remains
unusable until an authorized Administrator re-enters its credentials through an
Administration Plane. The [Log Module Design](log-modules/log-module-design.md)
defines the destination-specific recovery and capacity requirements.

The Application Database is not a module. It MUST NOT be enabled, disabled, or
changed after deployment initialization. Weavelit MUST NOT support in-place
migration between Application Database technologies. Database backends MUST be
compiled into the Server package and MUST NOT be runtime-installable plugins.

## Pre-Operational Lifecycle

Before Init or Restore completes, the Server MUST operate in a restricted,
uninitialized mode. Init and Restore MUST be mutually exclusive paths to an
initialized state and MUST NOT require an existing Administrator. The Server
MUST expose each path only through a
**[Pre-Operational Surface](glossary.md#applications-and-interfaces)** whose
Client Module explicitly declares the matching capability.

### Operating Responsibility And Lifecycle Interruption

Weavelit delivers its defined application workflows; the deployment operator
is responsible for host availability and maintenance, power, installation and
deployment execution, environment validity, backup custody, and recovery
material retained outside Weavelit. Weavelit does not promise recovery or
survival of application state, Log Module records, or an acknowledged record
across host power loss, filesystem loss or corruption, abrupt process
termination, or an operator-broken environment. A durable acknowledgement is
not such a survival guarantee. Where the Server can start and classify the
state, it provides fail-closed behavior and only a stable, redacted
operator-action notice; it does not claim post-failure health or add special
recovery, retry, reconciliation, or filesystem-persistence hardening solely to
guarantee acknowledged-record survival after those host failures. This boundary
does not make every error an operator responsibility: within a valid supported
running environment, the Server remains responsible for its defined
correctness, validation of untrusted clients and data, authentication,
authorization, secret handling, safe normal-run behavior, and protection
against unsafe automatic overwrite.

When an Init or Restore interruption leaves retained partial lifecycle state,
the Server MUST classify that state, remain fail-closed and non-operational,
and emit only a stable, redacted action-class diagnostic. It MUST NOT reconcile,
resume, retry, complete logging for, seal, delete, recreate, or otherwise make
the retained state usable. It MUST NOT expose normal operation or a
Pre-Operational fallback over that ambiguous state.

How the Server MUST treat a SQLite write-ahead log (WAL) in retained
**[Application Database](glossary.md#applications-and-interfaces)** state
depends on the deployment's recorded lifecycle state, because the two are
classified by different means. A pre-operational deployment is classified by
non-mutating inspection that deliberately cannot reconcile a WAL and would
otherwise read stale main-file state, so it must refuse. An initialized
deployment is classified from its deployment record alone and then verified by
the authoritative read-write open that loads it, which lets SQLite recover the
WAL as it normally does.

When the deployment record is uninitialized or has an initialization pending
and retained Application Database state includes a WAL, the Server MUST classify
it as `lifecycle_interrupted` / `operator_redeploy_required` without opening,
copying, inspecting, recovering, checkpointing, or otherwise modifying the
original database, WAL, or shared-memory artifacts. It MUST NOT derive an
Init- or Restore-specific action from WAL-bearing retained state.

When the deployment record is initialized, the Server MUST NOT classify the
deployment through that non-mutating inspection, and MUST NOT treat the presence
of a WAL or any other SQLite recovery sidecar as a redeployment condition. It
MUST load the sealed deployment through its authoritative read-write open,
letting SQLite recover the WAL, and MUST NOT delete or otherwise alter WAL or
shared-memory artifacts itself. That load remains the authority and MUST fail
startup closed, before any listener binds, when the database cannot be opened or
recovered, fails integrity or schema validation, is bound to a different
deployment, or holds incomplete or unacknowledged initialized state. The Server
MUST NOT report any of those failures as `operator_redeploy_required`.

On a signalled shutdown, the Server MUST wait for every admitted irreversible
lifecycle transition and every Application Database close it begins to finish
before its runtime tears down. The 300-second lifecycle-transition and
five-second database-close thresholds report an incomplete shutdown when
exceeded, but MUST NOT authorize abandoning the transition or close. A
supervisor that requires this graceful-stop guarantee MUST use `SIGTERM` and
MUST NOT impose a finite force-kill timeout.

The operator MAY preserve the failed state root for diagnosis or evidence, or
discard it and rebuild or redeploy the host. A failed fresh Init requires a new
deployment and a new Init. A failed Restore requires a replacement deployment
and a new Restore attempt using independently retained compatible backup and
recovery material; Weavelit does not retain that material or manage its
durability. Operator-directed destruction requires a separately documented
procedure and boundary before the Server may support it.

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

Before either workflow changes application state, a Client Module's
**[Pre-Operational Surface](glossary.md#applications-and-interfaces)** with the
applicable capability MUST use the shared pre-operational Server contract to
select and configure the Application Database. The client MAY submit only the
selected backend and its declared typed connection values; it MUST NOT submit or
influence a filesystem path for the database, locator, credential storage, or
another Server-managed artifact. The Server MUST derive every local path and
MUST exclusively create, place, protect, replace, and remove every local file
required by the selected backend. The Server MUST validate the selection and
MUST persist in protected Server-local configuration only the backend
identifier, non-secret connection settings, and encrypted secret connection
values needed to reopen the selected database. Secret connection values MUST
terminate in Server-owned protected credential handling and MUST NOT be
persisted as caller-supplied references or plaintext configuration. The Server
MUST then open the Application Database without requiring a restart. All other
application-owned configuration MUST be stored in the Application Database.

Application Database selection and Init or Restore MUST occur through a Client
Module's Pre-Operational Surface that declares the applicable capability.
Package installation, service configuration, and container adapters MUST supply
only the host and process settings needed to start the Server in
restricted uninitialized mode. They MUST NOT select the Application Database or
create a second application-configuration surface. Init MUST create, or Restore
MUST import, application configuration; authenticated
**[Administration Plane](glossary.md#applications-and-interfaces)** functions
MUST own subsequent mutable application settings.

### Deployment Binding And Sealing

On first startup, the Server MUST create a protected Server-local deployment
record containing a unique deployment identifier and lifecycle state. The
shared lifecycle MUST bind the Application Database locator and every pending
or initialized database state to that identifier. Before Init or Restore
commits initialized state, the record MUST enter `InitializationPending`. After
the database commit and completion of every required workflow obligation during
the same valid workflow run, the Server MUST irreversibly seal the record as
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
and can provide the durable acknowledgement defined in
[Logging And Accountability](#logging-and-accountability) for their assigned
log type. After the Application Database commit, the Server MUST receive that
acknowledgement for the Init result through the committed System Log assignment
before sealing. Successful Init MUST then transition the running Server directly
to normal operation without a restart.

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
deployment identifier, receive the durable acknowledgement defined in
[Logging And Accountability](#logging-and-accountability) for the Restore result
through the restored System Log assignment, seal the replacement deployment,
and transition the running Server directly to normal operation without a
restart. Restore MUST NOT support in-place migration between Application
Database technologies.

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
machine. It MUST support macOS 26 or newer on Apple Silicon (`arm64`). During
normal operation, the Weavelit CLI Client Module MUST
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
26.04 LTS `amd64` host, where it runs as a gateway service.

## Additional Delivery And Extension Requirements

The Server MUST provide a supported OCI-compliant production image containing
the same versioned, prebuilt Server release output used to assemble the `.deb`
package. The image MUST be a sibling delivery wrapper rather than a separate
Server build, and it MUST NOT compile the application when the container starts.

Weavelit MUST offer MCP adapters through separate Client Modules that use the
same supported Operation contracts as other clients.

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
- [Server Audit Log Design](server/audit/audit-log-design.md)
- [Testing and Validation Policy](testing.md)
- [Audit Terminal Binding Retention And Supersession Decision](log-modules/audit-terminal-binding-retention-decision.md)
