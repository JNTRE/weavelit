# Log Module Design

This document defines the shared design for Server-side
**[Log Modules](../glossary.md#applications-and-interfaces)**. It does not
define a destination-specific storage or delivery implementation. It defines
the recovery and retention-policy boundaries that every destination must
preserve.

## Contract And Delivery Boundary

`weavelit-server-log` is the Server-owned shared contract and dispatch crate.
It defines a bounded typed record envelope with distinct System and Audit
variants, declared module capabilities, trusted registration and factory
inputs, process-level durable-delivery acknowledgement, and payload-free typed
errors. The
common envelope includes a Server-generated opaque record identifier, event
time, result, and correlation identifier. It contains no SQLite, filesystem,
Application Database, client-wire serialization, query, retention, backup,
recovery, purge, or remote-credential behavior.

The contract enforces UTF-8 byte limits before it constructs a complete
record. Every record carries a nonzero 16-byte random `record_id`, a UTC
Unix-millisecond `event_time`, a `success` or `failure` `result`, a
`correlation_id` of 1–64 bytes, and a `classification` selected from its
closed typed catalog of lowercase dotted identifiers. A System record adds a
`detail` of 1–4096 bytes; an Audit record adds a typed `principal` of 1–256
bytes that is structurally either human with no
**[Responsible Owner](../glossary.md#identities-and-access)** or automation
with its required 1–256-byte `responsible_owner`, plus an `action` of 1–128
bytes, a `target` of 1–1024 bytes, and its own `detail` of 1–4096 bytes. Every
field is pre-redacted before construction, and the correlation identifier plus
every body field is at most 8 KiB combined. Empty and oversized values are
rejected without truncation, hashing, raw source payload retention, or a
replacement record. Audit and Observability are the only producers of these
pre-redacted bounded summaries; a logging-required workflow fails if it cannot
construct one, and a destination receives no unbounded or partial record.

Its compiled-in catalog validates each registration before invoking its factory
with trusted Server context. A configured destination accepts only a complete
immutable `CompleteLogRecord`; no public delivery operation accepts a raw
source payload or a caller-created record identifier. The destination must
acknowledge the same identifier and type synchronously after completing its
configured supported storage interface's commit path during a valid process run
or confirming an exact prior-record match. A capability mismatch, malformed
registration,
unavailable destination, or conflicting replay returns a stable payload-free
error.

The Server-owned contract and dispatch boundary retains the trusted context
that configures a catalog destination and creates record issuers, read-only
factory context, configured dispatch, and durable acknowledgement capabilities.
An ordinary compiled-in Log Module can register its declared capabilities and
receive only a read-only factory context, a complete assigned record, and the
one-use acknowledgement capability that dispatch supplies for that record. It
cannot mint a record identity or trusted context, construct an acknowledgement,
turn factory context into trusted context to configure a catalog destination,
or inject a configured dispatch. A validated catalog without the Server-retained
trusted context has no configured destination or delivery authority. Isolated
SQLite destination tests construct only private SQLite-owned persistence inputs;
normal module dependency graphs contain no authority-minting test feature and do
not depend on Server or lifecycle crates. External-consumer compile fixtures prove
provide stable boundary evidence for both the permitted registration surface and rejection of issuer, context,
acknowledgement, direct dispatch, and catalog-mediated destination-configuration
attempts.

Server Audit constructs and pre-redacts Audit records; Server Observability
constructs and pre-redacts System records, including Init and Restore completion
results. A Log Module accepts only these complete typed records. It may validate
its declared capability and persist or deliver a record, but it must not redact,
enrich, reinterpret, or access Application Database state.

Delivery is synchronous and succeeds only when the assigned destination
completes its configured supported storage interface's commit path for the
complete record or confirms an exact existing record with the same type and
record identifier. Before an Init or Restore application-state commit, the
Server creates the opaque identifier and persists it with immutable
completion-record fields in the post-commit obligation. During a valid
uninterrupted workflow run, the Server delivers that identical record until it
receives a durable acknowledgement. That acknowledgement does not guarantee
record survival across host power loss, filesystem loss or corruption, abrupt
process termination, or an operator-broken environment. An interruption before
acknowledgement leaves the workflow non-operational; the lifecycle does not
retry delivery, construct a replacement record, or seal on restart. A matching
identifier with different content is an integrity failure. This provides
at-least-once attempts and one persisted completion record per identifier for
the MVP SQLite destination; it does not claim distributed exactly-once delivery
or select a fan-out policy.

**Operational Resilience Requirement:** The Server does NOT crash or exit if a
Log Module destination becomes unavailable during normal operation (after
Init/Restore). Instead:

- Delivery requests fail with a stable, payload-free error
  (`LogDeliveryError::Destination`).
- For **consequential operations** (those whose failure must be auditable), the
  route layer propagates the delivery error, causing the operation to fail with
  a stable error message: "Audit Log unavailable; operation rejected."
- For **non-consequential operations** (read-only queries, health checks, or
  internal-only tasks), the route layer MAY absorb the delivery failure and
  succeed, after recording the failure in System Logs.

A "consequential operation" is an operation that modifies application state
(account creation, permission grant, policy change, etc.). All Administration
Plane mutations are consequential and MUST fail if Audit Log delivery fails.

## Destination Preflight And Configuration Validation

`LogDestination` declares `preflight` as a required trait method rather than a
defaulted one, so a Log Module cannot be implemented without deciding how it
proves that its configured destination can complete its commit path for a
given record type before the Server relies on it for delivery.

`ConfiguredLogDestination::preflight` checks the Server-declared capability
before it delegates to the module: an assignment for a record type the module
does not declare returns `LogDeliveryError::CapabilityUnavailable` without
reaching the module at all. Only a declared record type reaches the module's
own `preflight`, whose failure surfaces as `LogDeliveryError::Destination`
carrying the module's stable, payload-free `LogDestinationError`.

A valid preflight proof must exercise the same commit path that delivery later
uses for that record type, and it must leave no persisted record behind on
either outcome. A proof that only checks reachability, or that writes a record
it does not remove, does not satisfy this contract.

The trusted context a catalog destination receives also carries the
destination's committed, non-secret configuration settings. A Log Module's
factory must reject any setting it does not define as
`LogDestinationError::ConfigurationInvalid`; `LogModuleCatalog::create_destination`
propagates that rejection as `LogConfigurationError::Destination`, so an
unconfigured or misconfigured setting is refused rather than silently ignored.

**Runtime vs. Configuration-Time Failures:** If preflight fails during Init or
Restore, the Server MUST fail closed (Init/Restore fails, no deployment
proceeds). If a destination becomes unavailable **after** Init succeeds and
normal operation has begun, the failure is handled at the route layer (see
"Operational Resilience Requirement" above); the Server remains operational.

### Declaring The Settings A Module Accepts

A module states which settings it defines exactly once, through
`LogDestinationFactory::accepted_settings`. Like `preflight`, the method is
required rather than defaulted, so a Log Module cannot be implemented without
deciding which settings it accepts. It returns a `LogSettingsContract`: a
validated, ordered set of the setting keys the module defines, bounded by the
same `MAX_DESTINATION_SETTINGS` count and `MAX_DESTINATION_SETTING_KEY_BYTES`
key length that bound a committed configuration. A module that derives its whole
destination from the trusted context, as the compiled-in `sqlite` module does,
declares `LogSettingsContract::none()`.

`LogModuleCatalog::new` reads that declaration from each registered factory and
carries it on the module's validated `LogModuleDeclaration`, alongside its
identifier and capabilities. `LogSettingsContract::accepts` then judges a
configuration's settings against the module's own declaration. That comparison
is pure: it inspects declared keys, opens no destination, and creates no local
storage, so a caller may judge a configuration before anything durable exists.

The declaration and the factory's own refusal are the same statement. A module
refuses the settings it is handed by testing them against `accepted_settings`
rather than by restating the rule inside its factory, so a configuration a
caller judged as acceptable cannot be one the module then refuses to open, and a
caller can never judge a configuration by a rule the module does not apply.
This seam is what lets **[Init](../glossary.md#states-and-requests)** and
**[Restore](../glossary.md#states-and-requests)** refuse a configuration
carrying settings its named module could not serve before their checkpoints are
committed, without opening the destination; see
[Compiled-In Component Inventory](../server/lifecycle/restore/restore-design.md#compiled-in-component-inventory).

## Event Classification Taxonomy

Every classification is a lowercase dotted identifier that a producer selects
from its closed typed catalog; a destination stores its canonical string
opaquely and does not enforce the taxonomy. The initial System Log taxonomy is
`lifecycle.startup`, `lifecycle.init`, `lifecycle.restore`,
`operational.state`, `configuration.change`, `authentication.failure`,
`authorization.denial`, `dependency.failure`, `provider.failure`, and
`internal.error`. The initial Audit Log taxonomy is `lifecycle.backup.created`;
`authentication.user.created`, `authentication.user.disabled`,
`authentication.password.changed`, `authentication.password-reset.started`,
`authentication.mfa.enrolled`, `authentication.mfa.reset`,
`authentication.mfa-requirement.changed`,
`authentication.mfa-module-enablement.changed`, and
`authentication.session.revoked`; `authorization.group.created`,
`authorization.group-membership.changed`, `authorization.group-grant.changed`,
and `authorization.automation-scope.changed`;
`dependency.log-module-configuration.changed` and
`dependency.service-connection.changed`; `provider.operation.started` and
`provider.operation.completed`; and `internal.server-configuration.changed`,
`internal.user-status.changed`, and `internal.log-policy.changed`.

- **`dependency.audit-log-unavailable`** (System Log)
  - Recorded when a Log Module destination fails to accept an Audit Log record.
  - Recorded when a consequential operation fails because Audit Log delivery
    was unavailable.
  - Recorded when a non-consequential operation absorbs an audit-delivery
    failure.
  - Context: timestamp, destination module name, reason for failure (if
    available), affected operation (if applicable).

- **`authorization.group-grant.removal-denied`** (Audit Log)
  - Recorded when an Administrator attempts to remove the last
    ServerAdministration grant from an account and the operation is rejected.
  - Context: administrator (principal), target group, target account, reason
    (e.g., "would orphan permission").

**[Init](../glossary.md#states-and-requests)** and
**[Restore](../glossary.md#states-and-requests)** completion results remain
System-only events under `lifecycle.init` and `lifecycle.restore`. A raw
dependency, provider, authorization, or internal failure is a System event; a
consequential authenticated action that follows from it may additionally
produce an Audit event. This taxonomy does not change the Log Module
non-enrichment boundary: a destination never adds a field to a record it
receives.

## MVP SQLite Destination

`weavelit-module-log-sqlite` is the compiled-in MVP destination implementation.
It exclusively owns the Server-derived deployment-local destination at
`WEAVELIT_STATE_ROOT/log.sqlite3`, including recognized SQLite sidecars,
deployment binding, schema and migration ledger, connection, transactions,
health checks, locking behavior, and redacted error mapping. No client contract
accepts a path, filename, URI, or connection string for this destination.

The runtime constructs one validated SQLite registration in its compiled-in Log
Module catalog after lifecycle startup classification and retains that catalog
for the process lifetime. Catalog construction does not invoke the destination
factory. Until a later Server-owned configuration and assignment flow selects
the module, startup neither opens nor delivers to the destination.

The destination stores System and Audit records separately within its own
database. It must not depend on or reuse an Application Database crate, file,
schema, connection, configuration, or resource. It may use the same
workspace-pinned `rusqlite` package after dependency review. Init preflights
the System Log and Audit Log destinations before its application-state commit,
keeps each destination for the process lifetime, and validates it during
startup without post-commit reconciliation delivery. Restore does not preflight
either destination: preflight proves a commit path by writing and deleting a
durable probe row, and Restore guarantees that a pre-checkpoint failure leaves
nothing behind, so Restore instead resolves the assigned System Log module's
identifier and confirms the named module's availability before its point of no
return, then opens that destination only after the checkpoint completes.
Restore imports Module configuration and assignments, never destination data.

This destination proves its preflight commit path by writing a probe row
through the exact delivery commit path — the same immediate transaction and
commit that delivery uses for that record type — then deleting the row within
that same transaction, so storage that is read-only, out of space, or
schema-incompatible is refused and no probe record survives either outcome.
The probe uses a reserved all-zero record identifier; `TrustedRecordIssuer::issue`
refuses to issue an all-zero identifier, so no genuine record can ever share
it. This destination defines no destination configuration setting, so it
refuses any setting a configuration supplies as
`LogDestinationError::ConfigurationInvalid` rather than accepting or silently
ignoring it.

This MVP defines one local SQLite destination rather than Server-issued
multiple destination instances. The recovery and capacity policy below defines
requirements for future destination implementation work; it does not add
backup, recovery, retention, purge, or capacity behavior to the current
unselected SQLite catalog scaffold, assert that it is production-ready, or
activate it through a configuration or assignment flow.

The SQLite destination derives its fixed `log.sqlite3` filename only from the
trusted local root supplied to its factory; it does not inspect an environment
variable or accept a client path. It opens the destination without following a
database-file symlink, uses SQLite write-ahead logging with full synchronous
commit behavior as its configured valid-run commit path, and serializes its
owned connection. That commit path does not provide a host-failure survival
guarantee. Transient SQLite sidecars remain part of this destination's owned
resource set when SQLite creates them. It stores the complete fields of System
and Audit records in separate typed tables rather than exposing a serialized
record blob.

Freshness requires the absence of every recognized artifact: `log.sqlite3`,
`log.sqlite3-journal`, `log.sqlite3-wal`, and `log.sqlite3-shm`. When the main
database is absent but any recognized sidecar exists, the destination must fail
closed with an integrity failure before reserving or opening the main database,
configuring SQLite, or validating the binding, ledger, or schema. It must not
alter an orphan sidecar. When the main database exists, its binding, ordered
checksummed migration ledger, expected ledger-prefix schema, and SQLite
sidecars retain their existing validation and recovery behavior. A missing,
empty, malformed, duplicate, mismatched, unknown, reordered, changed, or
schema-mutated artifact fails closed without altering the destination. Fresh
bootstrap atomically creates migration 1, its ledger entry, and the matching
binding before later migrations may run. Opening, health, lock, and delivery
failures map only to the shared payload-free destination errors; they do not
disclose paths, SQL, record contents, or secrets.

SQLite stores the bounded record fields with byte-based `CAST(... AS BLOB)`
constraints and the same aggregate 8 KiB maximum. A migration that adds these
constraints rebuilds its tables transactionally and copies existing records
only when they satisfy the new schema. An existing oversized row makes the
migration fail closed without dropping, altering, or replacing any destination
record; this MVP defines no data-recovery exception for that incompatibility.

The migration ledger's existing checksummed migrations 1 and 2 remain
unchanged. Migration 3 transactionally adds nullable `classification`,
`principal_type`, and `responsible_owner` columns to the Audit table. Existing
rows remain unchanged with all three new fields `NULL`. A newly written Audit
record always supplies its canonical classification and principal type; its
typed principal supplies no `responsible_owner` for a human and the required
one for automation. The shared record contract enforces that structural rule
before construction; SQLite stores the resulting canonical strings without
enforcing the classification taxonomy.

### Descriptor-Relative Candidate Evidence

On supported Ubuntu, an isolated real-filesystem probe held a root-directory
descriptor, renamed its former pathname, and recreated that pathname as a
replacement directory. The unselected
`/proc/self/fd/<held-root-fd>/log.sqlite3` candidate resolved fresh-artifact
preflight and reservation beneath the held directory, with no recognized
artifact created in the replacement directory. The failed fresh attempt leaves
only its zero-length main-file reservation in the held directory. Pinned
`rusqlite` 0.40.1 with bundled SQLite then rejected the main database open with
`SQLITE_CANTOPEN_SYMLINK` under the required `SQLITE_OPEN_NOFOLLOW` policy,
before WAL or SHM creation. An existing destination similarly remained
unchanged when reopened through that candidate; orphan sidecars were found
beneath the held directory and rejected as integrity failures; and a final
database-file symlink remained rejected without following its target. An
unavailable descriptor root maps only to the payload-free unavailable error.

This is supported-environment feasibility evidence, not a selected root-handle
propagation mechanism. It does not authorize a change to the trusted-context
contract, the symlink policy, or SQLite's VFS behavior.

## Destination Recovery And Retirement

Each destination owns its protection, snapshot or backup, migration,
compatibility, recovery, and retirement behavior. A replacement destination
must create or validate destination lineage. An unknown, corrupt, mismatched,
or incompatible artifact must fail closed with a stable, redacted error; a
destination must never automatically reset, overwrite, or delete it.

SQLite destination protection and copying must use a destination-owned,
SQLite-consistent snapshot procedure. Copying a lone WAL-mode database file is
not a valid snapshot or recovery procedure. A future remote destination must
choose either source-bound replacement lineage or validated shared continuity
with exact record-identifier replay.

## Destination Retention And Capacity

Retention and purge are destination-owned and Administrator-selected only when
the destination declares them relevant. A destination, including an email
destination, may declare retention unsupported. The Server does not provide an
automatic Server-wide purge or arbitrary global retention default.

SQLite capacity protection is opt-in and disabled by default. When an
authorized Administrator enables it, the SQLite destination validates and uses
a module-specific page budget, filesystem and WAL reserve, and System Log purge
rule and target. It must inspect its actual runtime page size and page ceiling
and reject an invalid budget. Before reaching its hard budget, it may perform
only the configured destination-owned purge and checkpoint work. It must not
perform automatic `VACUUM`.

At the hard budget or on SQLite `FULL`, the SQLite destination must stop durable
delivery with a stable, payload-free unavailable error. It must not delete
additional records unless the configured policy permits that deletion. System
Logs may be purged only through the configured SQLite policy. Audit Logs must
never be automatically purged; any Audit Log retention or deletion capability
requires a future explicit authorized decision that includes a hold policy.

A future Server administration contract owns authorization, confirmation,
policy, run, and status APIs and the Audit Logs for policy changes and purge
start, failure, and completion. This policy does not introduce purge behavior
to the current unselected SQLite catalog scaffold.

## Init And Restore Configuration

During **[Init](../glossary.md#states-and-requests)**, the person completing the
workflow uses an Init-capable
**[Pre-Operational Surface](../glossary.md#applications-and-interfaces)**
provided by a Client Module to select, configure, and activate one or more Log
Modules before assigning destinations for the two log types.

The **[Application Database](../glossary.md#applications-and-interfaces)** is
selected and configured by the shared lifecycle contract before Init accepts
Log Module configuration. Selecting the same underlying technology for an
Application Database and a Log Module does not reuse Weavelit-owned persistence
logic or implementation crates, configuration, database file, schema,
connection, or other resources. They may use the same workspace-pinned
third-party dependency, such as `rusqlite`, without sharing persistence
behavior. A Log Module may instead deliver records to a non-database
destination, such as email, an API endpoint, or Checkmk; its destination type
does not affect Application Database behavior.

The Init contract collects configuration in this order:

1. Select and configure a Log Module for **[System Logs](../glossary.md#applications-and-interfaces)**.
2. Assign that configured Log Module to receive System Logs.
3. Select and configure a Log Module for **[Audit Logs](../glossary.md#applications-and-interfaces)**.
4. Assign that configured Log Module to receive Audit Logs.

The person completing Init may select the same configured Log Module for both
assignments. Every Init-capable Client Module submits the same module
configuration and two explicit assignments to the same Server-owned
validation; no client defines an alternative Log Module initialization path.

Init rejects an absent, disabled, unconfigured, or incompatible assignment. It
also rejects any submitted configuration carrying a non-secret setting its named
Log Module does not declare, judged against the module's own declared settings
format on the compiled-in component inventory rather than against a rule Init
restates. That rejection is part of request validation, so it is correctable:
the person completing Init fixes the setting and retries the same finalization
request. Reaching the module's factory with an undeclared setting instead would
fail after the request was accepted, which is not a correctable outcome. Secret
settings are outside the declaration and are never carried to a module through
it, so they are not judged against it. Init also rejects an assignment unless
its configured Log Module can complete its configured supported storage
interface's commit path for the assigned log type, proven through the preflight
contract described in
[Destination Preflight And Configuration Validation](#destination-preflight-and-configuration-validation).
After Init commits application state, it receives a durable acknowledgement for
the Init completion result through the committed System Log assignment before
the deployment is sealed. Init remains incomplete, and the Server does not begin
normal operation, until both assignments are valid and the completion result is
acknowledged during that valid run.

During **[Restore](../glossary.md#states-and-requests)**, the Server imports
only non-secret Log Module configurations, enabled state, and assignments from
the validated Application Database backup. It does not import System Log or
Audit Log destination data or authentication or connection credentials. A
restored remote destination remains unusable until an authorized Administrator
re-enters its credentials through an
**[Administration Plane](../glossary.md#applications-and-interfaces)**. Every
referenced Log Module must be compiled into the replacement Server, and every
restored configuration and assignment must satisfy the same Server-owned
validation used during normal administration.

Restore validates both restored assignments and does not seal the replacement
deployment until the restored System Log assignment provides a durable
acknowledgement for the required Restore result without recovery secrets or
backup contents. That validation confirms each assigned Log Module is compiled
into the replacement Server; it does not prove the Audit Log destination can
complete its commit path, because Restore never opens or delivers to it. This
is a documented limitation of the Restore path rather than a defect: an
imported Audit Log assignment that cannot commit surfaces only when Audit
logging is first attempted after Restore completes. A failure remains non-operational and follows the retained-state interruption boundary in the
[Server Restore Design](../server/lifecycle/restore/restore-design.md). A restored
Log Module never reads backup contents or Application Database state directly.

## Related Documents

- [Technical Specification](../spec.md)
- [Security Model](../security-model.md)
- [Open Questions](../open-questions.md)
- [Glossary](../glossary.md)
- [Application Database Design](../server/database/application-database-design.md)
- [Server Restore Design](../server/lifecycle/restore/restore-design.md)
