# Server Lifecycle Design

This document defines the shared Server-owned lifecycle boundary used before
the **[Weavelit Server](../../glossary.md#applications-and-interfaces)** enters
normal operation. It owns startup classification, the deployment record,
**[Application Database](../../glossary.md#applications-and-interfaces)** selection
and locator persistence, workflow arbitration, mutation serialization, and
irreversible sealing. The [Server Init Design](init/init-design.md) and
[Server Restore Design](restore/restore-design.md) own their distinct application-state
workflows.

## Crate And Runtime Boundary

`weavelit-server-lifecycle` is the internal base crate shared by
**[Init](../../glossary.md#states-and-requests)** and
**[Restore](../../glossary.md#states-and-requests)**. It owns the trusted lifecycle
state and operations that both workflows require. The Init and Restore crates
depend on it but do not depend on each other.

The `weavelit-server` runtime supplies the compiled-in Application Database
backend catalog, asks the lifecycle crate to classify trusted state, and exposes
only the routes permitted by that classification. Runtime routing is an
additional control rather than the authority for a mutation. Every mutating
lifecycle, Init, and Restore operation independently asks the lifecycle crate to
open and validate the deployment record, locator, selected database, deployment
identifier, and current workflow eligibility before reading request secrets or
backup content or causing side effects.

The lifecycle crate does not create initial users, configure initial Log
Modules, generate or accept private recovery keys, interpret backup contents,
or implement client presentation. Those responsibilities remain in their
workflow and Client Module boundaries.

The backend-neutral domain and catalog layer defines the three canonical
lifecycle states, the nonzero 16-byte locator generation, version 1
deployment-record and locator domain values, and the later startup capability
classifications. It reuses the Application Database contract's 16-byte
deployment identifier rather than defining another deployment identity.

The runtime builds `BackendCatalog` from compiled-in backend registrations. A
registration declares one lowercase kebab-case backend identifier, at most 64
lowercase kebab-case connection fields, each field's required scalar kind,
required or optional status, and trusted secret classification, plus an
`ApplicationDatabaseFactory`. Field identifiers containing a local path,
directory, or file-reference concept are invalid. Catalog construction rejects
empty, invalid, duplicate, or oversized registrations.

A submitted connection field contains its identifier, scalar value, and claimed
secret classification. Common validation rejects an unknown or duplicate
field, a missing required field, the wrong scalar kind, a classification that
does not match the trusted declaration, more than 64 fields, or a string or byte
value over 16 KiB before invoking a backend factory. Validated settings are
sorted by identifier, remain bound to the backend declaration that validated
them, and carry the declaration's classification. String, signed 64-bit
integer, boolean, and byte values are the only supported kinds. The
caller-selected input has no path or file-reference kind.

The factory receives local filesystem context only through
`TrustedBackendContext`, supplied by trusted Server policy, and receives client
values only through `ValidatedConnectionSettings`. It maps implementation
failures to payload-free lifecycle categories and returns a boxed
`ApplicationDatabase`. Public errors and diagnostic formatting contain no
identifier text, connection value, local path, raw backend failure, or factory
implementation detail.

`LifecycleStore` implements the protected anchor profile beneath that contract.
It opens one trusted root without following any path component, validates its
closed inventory, holds the process-lifetime lock, and creates or authenticates
the key, deployment record, and active locator before returning. It never opens
an Application Database. `FirstStartCreated`, `FirstStartResumed`, and
`Retained` report whether the store created both anchors, recovered the sole
permitted key-only interruption, or reopened retained state.

Validated request settings retain the catalog's trusted secret classifications.
Before locator persistence, the store converts them to backend-bound typed
field/value pairs without persisting a caller-controlled classification. The
complete locator payload is encrypted. A later database-selection operation
must revalidate retained field types and classifications against the current
runtime catalog before invoking its backend factory.

Locator replacement durably publishes an immutable generation, then atomically
replaces the deployment record pointer as the commit point, authenticates on
reopen, and removes the prior generation. Record replacement requires the same
deployment identifier and active locator generation. Both operations remain
persistence mechanisms; eligibility and backend preflight remain owned by the
selection and startup-classification work.

Raw locator creation and replacement require a non-exhaustive
`LocatorPersistencePermit`; raw record replacement requires a non-exhaustive
`RecordPersistencePermit`. Only code inside `weavelit-server-lifecycle` can
construct those capabilities. Later public selection and transition authorities
hold them after independently validating eligibility; other crates cannot call
the persistence primitives directly.

## Deployment Record And Database Locator

The accepted [Lifecycle Anchor Protection And Serialization Profile](lifecycle-anchor-profile-decision.md)
defines the security and compatibility boundary for the deployment key,
deployment record, and database locator.

### Process Identity And Trusted State Root

The runtime requires the non-secret `WEAVELIT_STATE_ROOT` environment variable
to name the protected Server state directory. Its value must be a normalized
absolute path with no `.` or `..` component. Starting at the filesystem root,
the lifecycle crate opens each path component without following symbolic links.
Every component must be a real directory; ancestor ownership and access modes
remain host policy, while the final directory must be owned by the process's
effective user and have exactly mode `0700`.

The Server must run as a non-root operating-system identity. The supported
Debian package must create and use the locked, non-login `weavelit` system user
and primary `weavelit` group, provision the state root for that identity, and
grant no supplementary group by default. A later package requirement may add a
narrow group for a separately documented host resource such as TLS material;
such a group receives no access to the state root. Development and container
profiles may use another dedicated non-root identity when that identity owns
its state root. The lifecycle implementation validates the non-root effective
user and ownership by that user rather than hard-coding an account name.

Missing or empty configuration, a non-normalized or relative path, a missing or
symbolic-link component, root execution, unexpected final ownership, or a final
mode other than `0700` fails startup before an application route is exposed.
The Server does not use a default or fallback state location and does not create
the root. Host deployment configuration may select only this root; no client or
application request may select the root or a child path.

The lifecycle crate holds the opened root directory handle for its lifetime and
performs child inspection and mutation relative to that handle. It creates or
opens `lifecycle.lock` without following links and obtains a non-blocking
exclusive lock before inspecting another child. The lock is held for the
process lifetime. A second Server process using the same root exits before
binding HTTPS. The pinned Rust standard library's `File::try_lock` supplies this
process lock; no third-party lock dependency is required.

Before creating a managed file, the process sets an owner-only `0077` umask.
Every managed child must be a regular non-symlink file owned by the effective
user, have exactly mode `0600`, and have exactly one hard link. The state-root
filesystem must provide same-directory atomic replacement, durable file and
directory synchronization, and advisory locking. A filesystem that cannot
provide those guarantees is unsupported; a missing or failed guarantee stops
startup or mutation without a reduced-durability mode.

### Version 1 State-Root Inventory

Version 1 permits only these code-owned entries beneath the state root:

| Entry | Owner |
| --- | --- |
| `lifecycle.lock` | Lifecycle process lock; its content is empty. |
| `lifecycle-key.json` | Lifecycle at-rest key file. |
| `deployment-record.json` | Active encrypted deployment record. |
| `database-locator-<generation>.json` | Immutable encrypted locator generation. |
| `application.sqlite3` | SQLite Application Database. |
| `application.sqlite3-journal` | SQLite rollback or crash-recovery journal. |
| `application.sqlite3-wal` | SQLite write-ahead log. |
| `application.sqlite3-shm` | SQLite WAL shared-memory index. |

`<generation>` is exactly 22 canonical unpadded URL-safe Base64 characters
decoding to a nonzero 16-byte value. Lifecycle writes may also leave one of
these recognized crash remnants, where `<temporary>` has the same 22-character
grammar and decodes to a nonzero 16-byte value:

```text
lifecycle-key.json.tmp-<temporary>
deployment-record.json.tmp-<temporary>
database-locator-<generation>.json.tmp-<temporary>
```

The root may contain at most 256 entries. Any other name, subdirectory, FIFO,
socket, device, symbolic link, unsafe hard link, wrong owner, wrong mode, or
excess entry fails startup. After the active anchor set authenticates, the
lifecycle crate removes recognized lifecycle temporary files and unreferenced
locator generations and synchronizes the directory before exposing a route. An
unsafe recognized entry or cleanup failure fails closed. The lifecycle crate
never removes SQLite's `-journal`, `-wal`, or `-shm` files; the SQLite backend
owns their validation and recovery.

Future Server releases and compiled-in backends may expand this closed
code-owned inventory. A binary that does not recognize a newer entry fails
closed rather than ignoring it. The SQLite backend receives exactly
`WEAVELIT_STATE_ROOT/application.sqlite3`; neither the backend schema nor any
client input contains this path. No other version 1 child path is configurable.

### Version 1 File Formats

All version 1 files use compact UTF-8 JSON with no insignificant whitespace or
trailing newline. Object fields appear in the order listed below, arrays retain
their defined order, and JSON integers use their shortest base-10 form. The
format writer uses the pinned compact `serde_json` serializer against the typed
version 1 models. A reader rejects duplicate or unknown fields, missing fields,
unknown enum or algorithm values, trailing content, invalid UTF-8, invalid
Unicode, non-canonical binary encodings, and unsupported versions. It then
serializes the typed value through the same writer and rejects input whose bytes
differ. Dependency updates must preserve the known-answer vectors or introduce
an explicit format migration.

Every binary field uses unpadded URL-safe Base64. A canonical value uses only
the URL-safe alphabet, contains no `=`, has valid zero trailing bits, and equals
the encoder output for its decoded bytes. The at-rest key is 32 bytes; a nonce
is 24 bytes; a deployment identifier and locator generation are each 16 bytes
and nonzero. XChaCha20-Poly1305 ciphertext uses combined mode and includes the
complete 16-byte authentication tag as its final bytes.

The key file contains these fields in order:

```json
{"format_version":1,"key_algorithm":"xchacha20-poly1305","key":"<32 bytes>"}
```

`format_version` is exactly `1`, `key_algorithm` is exactly
`xchacha20-poly1305`, and an all-zero decoded key is invalid. The encrypted
deployment-record and locator envelopes contain these fields in order:

```json
{"format_version":1,"aead_algorithm":"xchacha20-poly1305","nonce":"<24 bytes>","ciphertext":"<combined ciphertext and tag>"}
```

The deployment-record plaintext contains these fields in order:

```json
{"deployment_identifier":"<16 bytes>","lifecycle_state":"uninitialized","locator_generation":null}
```

`lifecycle_state` is exactly `uninitialized`, `initialization_pending`, or
`initialized`. `locator_generation` is either `null` while no database is
selected or the canonical 16-byte generation token for the active locator.
`initialization_pending` and `initialized` require an active locator.

The locator plaintext contains these fields in order:

```json
{"deployment_identifier":"<16 bytes>","locator_generation":"<16 bytes>","backend_identifier":"sqlite","settings":[]}
```

Backend and field identifiers are 1 through 64 ASCII bytes and match
`^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$`. Settings are sorted by field identifier and
identifiers are unique. Each setting has fields `field_identifier` and `value`
in that order. `value` has fields `type` and `value` in that order and uses one
of these exact forms:

```json
{"field_identifier":"host","value":{"type":"string","value":"example.invalid"}}
{"field_identifier":"port","value":{"type":"integer","value":443}}
{"field_identifier":"tls-enabled","value":{"type":"boolean","value":true}}
{"field_identifier":"credential","value":{"type":"bytes","value":"AAECAw"}}
```

Strings contain at most 16 KiB of UTF-8; byte values decode to at most 16 KiB;
integers are signed 64-bit values; booleans are JSON booleans. Null, floats,
arrays, nested caller-defined objects, paths, and file references are not
setting values. The runtime-supplied trusted backend catalog defines which
identifiers and value types a backend accepts and which values are secret. The
locator does not persist a caller-controlled secret flag because the complete
locator is encrypted and the trusted catalog remains authoritative.

The pre-read and post-decode limits are:

| Resource | Version 1 limit |
| --- | --- |
| State-root entries | 256 |
| `lifecycle-key.json` | 512 bytes |
| Deployment-record envelope | 4 KiB |
| Deployment-record plaintext | 1 KiB |
| Locator envelope | 64 KiB |
| Locator plaintext | 32 KiB |
| Locator settings | 64 |
| Backend or field identifier | 64 ASCII bytes |
| One decoded string or byte setting | 16 KiB |

File length is checked before allocation or parsing. Decoded ciphertext is
bounded by the applicable plaintext limit plus the 16-byte tag before
decryption; authenticated plaintext is bounded again before JSON parsing.

### Authenticated-Encryption Binding

The deployment record and locator are encrypted and authenticated as complete
payloads with XChaCha20-Poly1305 under the deployment key. Each write obtains a
fresh random 24-byte nonce directly from the operating system. Nonces do not use
a persistent counter and are not stored in the key file. The record associated
data is exactly these ASCII bytes:

```text
weavelit:lifecycle:deployment-record:v1
```

Locator associated data is the ASCII prefix below followed immediately by the
raw 16 generation bytes decoded from its filename:

```text
weavelit:lifecycle:database-locator:v1:
```

The locator filename is therefore authenticated without trusting an envelope
field. After authentication, the payload generation must match the filename and
the deployment record, and both payload deployment identifiers must match the
selected Application Database. Authentication completes before plaintext JSON
parsing. No format field selects arbitrary cryptography: version 1 accepts only
the exact algorithm values above.

### Creation And Crash-Safe Replacement

After obtaining the process lock and validating the root inventory, an empty
deployment creates state in this order:

1. Generate a nonzero random 32-byte key, write and publish
   `lifecycle-key.json`, and synchronize the root.
2. Generate a nonzero random 16-byte deployment identifier and fresh record
   nonce, write and publish an `Uninitialized` record with no locator, and
   synchronize the root.
3. Reopen and authenticate both files before exposing restricted status.

A valid key file with no record, locator, Application Database file, or SQLite
sidecar is the one resumable incomplete anchor set. Startup reuses that key and
creates a fresh deployment identifier and record. Any malformed key, record or
locator without a key, locator or database artifact without a record, or other
partial bootstrap combination fails closed and never causes key regeneration.

Each lifecycle write uses an artifact-specific temporary name with a fresh
nonzero random 16-byte suffix. It creates the temporary regular file
exclusively at mode `0600`, writes all canonical bytes, calls `sync_all`,
atomically renames it to the final same-directory name, and synchronizes the
state-root directory. Key and locator publication require the destination to be
absent; deployment-record replacement atomically replaces the prior record.
Any operation or synchronization failure returns a redacted failure and exposes
no route. A crash before rename leaves an ignorable recognized temporary file;
a crash after rename leaves the final complete file and is reconciled from the
directory state.

Database selection and replacement use prepare-then-commit ordering:

1. Fully preflight the target backend and database without changing the active
   locator.
2. Generate a fresh nonzero locator generation, write and durably publish its
   immutable locator file, then reopen and authenticate it.
3. Write and durably replace the deployment record so its
   `locator_generation` points to the new locator. This record replacement is
   the commit point.
4. Reopen and authenticate the active record and locator, remove every safe
   unreferenced locator generation, synchronize the root, and only then expose
   the selected database or another route.

A crash before the record commit leaves the old selection active and the new
locator as a removable orphan. A crash after the record commit leaves the new
selection active; the old locator is removed during reconciliation. If the
record points to a missing, unsafe, unauthentic, mismatched, or unavailable
locator, startup fails closed rather than falling back to another generation.
The lifecycle crate never scans for a usable locator or selects the newest
filename.

The deployment key remains fixed for the deployment. Milestone 1 does not
rotate it in place. A missing, malformed, corrupted, or wrong key is never
regenerated while another retained anchor exists. Permanent key loss requires a
new deployment and Restore from a valid encrypted backup using its separate
recovery private key. Without both, the protected deployment state is
unrecoverable.

The generation pointer detects interrupted replacement, mixed generations, and
independent locator replay. Coherent replacement of a complete older key,
record, and matching locator set by sufficient host authority remains outside
the Milestone 1 guarantee because the deployment has no external monotonic
anchor.

### Version Compatibility

A version 1 reader accepts and writes only version 1. A future release that
changes an anchor format, algorithm, key custody mechanism, inventory, or
canonical writer must explicitly list every older version it can read and
migrate. There is no standing promise to read the immediately previous or every
historical version. A migration must authenticate the old complete set, write
and synchronize the complete new set with a documented commit point, validate
it, and preserve fail-closed rollback behavior before deleting old state.

Neither anchor accepts plaintext secrets, environment interpolation, a
caller-selected path, or a caller-supplied file reference. Secret connection
values are accepted only through declared typed fields over HTTPS, protected
before persistence, decrypted only when opening the selected database, held in
zeroizing owned buffers where practical, and never returned or logged.

The deployment record has only these states:

```text
Uninitialized
InitializationPending
Initialized
```

The selected database distinguishes an Init checkpoint from a Restore
checkpoint while reporting both as non-operational pending state. Each pending
checkpoint carries the deployment identifier, a workflow discriminator, and
only the non-secret workflow metadata required for safe retry or reconciliation.
Workflow crates own the meaning and validation of their checkpoint metadata.

No supported operation transitions an `Initialized` record to another state.
`InitializationPending` may return to `Uninitialized` only through an explicit
workflow recovery operation after the lifecycle crate verifies that the
selected database contains no application state. Only the workflow identified
by the checkpoint may request that reset; the other workflow remains
unavailable. The lifecycle crate performs the shared record transition, and the
owning workflow discards its checkpoint and any workflow-owned temporary
artifacts according to its design. Complete removal of every persistent
deployment anchor by sufficient host authority may appear to the Server as a
new installation; preventing or detecting that destruction belongs to
deployment access control and monitoring.

## Startup Classification

Package installation supplies only host and process configuration needed to
start the Server, including its HTTPS listener, TLS material, and protected
Server state directory. The lifecycle crate classifies every startup before the
runtime exposes an application route:

### Trusted HTTPS Listener Configuration

Trusted host configuration supplies exactly one listener address and port plus
the filesystem paths to its PEM certificate and matching private key. Neither a
client request nor application configuration may create, alter, or select this
listener or TLS material. The runtime must validate the listener configuration
and read the configured material under the filesystem protections required by
the [Security Model](../../security-model.md#https-listener-and-pre-operational-surface-security-profile)
before it binds.

The runtime reads only these non-empty host environment variables:

| Variable | Accepted value |
| --- | --- |
| `WEAVELIT_HTTPS_LISTENER_ADDRESS` | One numeric IPv4 or IPv6 socket address with a nonzero port. |
| `WEAVELIT_TLS_CERTIFICATE_PATH` | An absolute path with no `.` or `..` component to a PEM certificate chain. |
| `WEAVELIT_TLS_PRIVATE_KEY_PATH` | An absolute path with no `.` or `..` component to one PEM private key. |

The runtime rejects a relative path, a `.` or `..` component, a symbolic-link
component, a non-regular file, a hard-linked file, a group- or world-writable
file, empty or oversized material, and an unreadable file. A private key must
also have no permissions for other users; host administration remains
responsible for ensuring any group granted key access is narrowly scoped to TLS
material. Certificate files contain only certificate PEM sections, private-key
files contain exactly one supported private-key PEM section, and the runtime
uses its direct TLS provider to verify the leaf certificate and private key form
a usable pair. The validation boundary reads at most 1 MiB from either file and
does not bind, reserve, or probe a socket; a later listener-composition boundary
treats a bind failure as fail-closed.

An absent, malformed, unreadable, unsafe, or mismatched certificate or private
key, or an invalid or unavailable listener address or port, fails startup
closed. The runtime then exposes no route, cleartext HTTP fallback, alternative
listener, Init or Restore recovery surface, application-configuration surface,
or unauthenticated administrative surface. Certificate issuance and renewal
remain host responsibilities; the runtime validates the material supplied at
each startup rather than issuing, renewing, or replacing it.

| Deployment record | Locator and database state | Server behavior |
| --- | --- | --- |
| Absent | Locator absent | Create an `Uninitialized` record, then expose restricted pre-operational status without a database selection. |
| Absent | Locator present | Fail startup closed without exposing Init or Restore. |
| `Uninitialized` | Locator absent | Expose restricted pre-operational status without a database selection. |
| `Uninitialized` | Matching locator and uninitialized database | Expose restricted pre-operational status with the selected database. |
| `Uninitialized` | Matching locator and Init checkpoint | Advance the record to `InitializationPending`, then expose only Init reconciliation. |
| `Uninitialized` | Matching locator and Restore checkpoint | Advance the record to `InitializationPending`, then expose only Restore reconciliation. |
| `Uninitialized` | Matching locator and initialized database | Fail startup closed because the combination violates finalization ordering. |
| `InitializationPending` | Matching locator and Init checkpoint | Expose only Init reconciliation and finalization. |
| `InitializationPending` | Matching locator and Restore checkpoint | Expose only Restore reconciliation and finalization. |
| `InitializationPending` | Matching locator and initialized database | Complete workflow-specific post-commit reconciliation and seal the record before normal operation. |
| `Initialized` | Matching locator and initialized database | Load application state and start normal authenticated operation. |
| Any existing record | Missing required locator, identifier mismatch, unexpected state combination, unsafe or malformed state, unavailable database, or integrity failure | Fail startup closed without exposing Init or Restore. |

An unavailable, missing, malformed, unsafe, mismatched, or integrity-failing
configured database never causes the Server to expose a pre-operational
workflow as fallback recovery. An `InitializationPending` deployment is
non-operational and exposes only the workflow identified by its checkpoint. It
does not enable login, administration, or normal client functions.

The Milestone 1 runtime maps the two uninitialized rows to the Web UI Client
Module's status-only Pre-Operational Surface. It removes that status route from
every pending, sealed, normal, and failed-startup classification. Pending Init
and Restore classifications retain the sole direct TLS listener but register no
functional route; every valid unmatched request receives the Client Module's
fixed JSON `404` result. The
[Web UI Pre-Operational Status Surface](../../client-modules/web-ui/pre-operational-status-design.md)
defines its public contract; this lifecycle boundary remains the authority for
whether the route exists.

## Application Database Selection

The lifecycle crate presents the runtime-supplied backend catalog and each
backend's typed connection fields through a Client Module's
**[Pre-Operational Surface](../../glossary.md#applications-and-interfaces)**
that declares an applicable capability. A client selects a backend and submits
only the values required by those fields. No backend schema exposes a local
filesystem path or file-reference field, and the client never selects the
database, deployment-record, locator, or credential-storage location.

Selection validates the common request structure, asks the selected backend to
validate its settings, protects secret connection values, opens the target, and
inspects its trusted state. An initialized, pending, unavailable, mismatched, or
integrity-failing target is not eligible. This preflight occurs before Init
accepts an Administrator password or Log Module credential and before Restore
accepts a backup or private recovery key.

After successful preflight, the lifecycle crate writes the protected locator
and opens the selected database without a process restart. The client may
replace the selection with another eligible database before either workflow
creates a pending checkpoint. Replacement is fully preflighted before atomic
locator replacement and does not delete artifacts at the previous destination.
Database selection becomes immutable when a pending checkpoint exists and
remains permanently immutable after sealing.

## Workflow Arbitration And Sealing

Init and Restore are mutually exclusive consumers of one selected uninitialized
database. The lifecycle crate grants an exclusive workflow mutation permit only
after rechecking current trusted state. Creating either workflow checkpoint
makes the other workflow unavailable. Concurrent or stale requests are
rechecked while serialized; at most one workflow can commit application state.

The lifecycle progression is:

```text
Uninitialized -> DatabaseSelected -> InitializationPending -> Initialized
```

The deployment record remains `Uninitialized` through `DatabaseSelected`.
Before either workflow commits application state, its database checkpoint and
the deployment record become `InitializationPending` using crash-safe ordering.
The Application Database performs the workflow's complete state replacement
atomically and remains the final one-time guard. The committed state carries a
workflow-specific System Log completion obligation with non-secret event fields.
After that commit, the owning workflow durably delivers the completion result
through the committed System Log assignment and marks the obligation complete.
Only then does the lifecycle crate seal the deployment record `Initialized`.
Only after the seal is durable may the runtime remove all pre-operational routes,
load application state, and enable normal authenticated operation in the same
process.

If database state commits but sealing or in-process activation fails, the
runtime exposes no routes and fails closed. On restart, the lifecycle crate
verifies the matching initialized state, completes any workflow-specific
post-commit requirement, seals the record, and only then permits normal
operation. The lifecycle crate does not interpret a workflow-specific
obligation: it invokes the workflow crate that created it and seals only after
that crate reports durable completion. Init and Restore each own their required
System Log result. Reconciliation retries incomplete completion logging and does
not seal until the result is durable. Neither Init nor Restore is exposed again.

## Errors And Sensitive Output

Lifecycle failures use the Server's centralized typed error presentation.
Before HTTPS binds, a startup failure writes exactly one compact JSON object
containing `category` and `reason` string fields to standard error and exits
with status `1`. No diagnostic listener or fallback recovery surface starts.
Request-time lifecycle failures may carry the same pair through centralized
typed error presentation. The allowed pairs are:

| Category | Reasons |
| --- | --- |
| `configuration_invalid` | `listener_not_configured`, `listener_address_invalid`, `tls_certificate_not_configured`, `tls_private_key_not_configured`, `tls_material_invalid`, `state_root_not_configured`, `state_root_path_invalid` |
| `preoperational_unavailable` | `state_root_in_use`, `https_listener_unavailable` |
| `storage_unavailable` | `storage_operation_failed`, `database_unavailable` |
| `storage_integrity_failure` | `anchor_set_invalid`, `anchor_version_unsupported`, `anchor_binding_invalid`, `database_integrity_failure` |
| `deployment_state_invalid` | `state_combination_invalid` |

For example, a missing environment variable produces exactly:

```json
{"category":"configuration_invalid","reason":"state_root_not_configured"}
```

The pair has no dynamic payload. Diagnostics, client output, `Display`, and
`Debug` never include deployment or generation identifiers, backend or field
identifiers, key bytes, nonces, tags, ciphertext, plaintext fields or values,
file presence, filenames, byte counts, filesystem paths, raw Rust or dependency
errors, SQL, or operating-system details. `already_initialized` remains a
normal stable application category after trusted state has been opened; it is
not an anchor-startup failure.

The runtime composes routes and binds its sole direct TLS listener only after
trusted TLS configuration validation and lifecycle classification. A router,
TLS-listener, or bind setup failure exits with status `1` and exactly this
standard-error pair before a route is exposed:

```json
{"category":"preoperational_unavailable","reason":"https_listener_unavailable"}
```

## Version 1 Known-Answer Vector

The version 1 format and cryptographic test vector below is public test material,
not a production secret. The fixed key bytes are hexadecimal `00` through `1f`,
the deployment identifier is `10` through `1f`, the locator generation is `20`
through `2f`, the record nonce is `30` through `47`, and the locator nonce is
`48` through `5f`.

| Value | Canonical representation |
| --- | --- |
| Deployment identifier | `EBESExQVFhcYGRobHB0eHw` |
| Locator generation | `ICEiIyQlJicoKSorLC0uLw` |
| Locator filename | `database-locator-ICEiIyQlJicoKSorLC0uLw.json` |
| Record nonce | `MDEyMzQ1Njc4OTo7PD0-P0BBQkNERUZH` |
| Locator nonce | `SElKS0xNTk9QUVJTVFVWV1hZWltcXV5f` |
| Record associated data | ASCII `weavelit:lifecycle:deployment-record:v1` |
| Locator associated data | Hex `77656176656c69743a6c6966656379636c653a64617461626173652d6c6f6361746f723a76313a202122232425262728292a2b2c2d2e2f` |

The canonical record plaintext is:

```json
{"deployment_identifier":"EBESExQVFhcYGRobHB0eHw","lifecycle_state":"uninitialized","locator_generation":"ICEiIyQlJicoKSorLC0uLw"}
```

The canonical locator plaintext is:

```json
{"deployment_identifier":"EBESExQVFhcYGRobHB0eHw","locator_generation":"ICEiIyQlJicoKSorLC0uLw","backend_identifier":"sqlite","settings":[]}
```

The canonical `lifecycle-key.json` bytes are:

```json
{"format_version":1,"key_algorithm":"xchacha20-poly1305","key":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"}
```

The canonical `deployment-record.json` bytes are:

```json
{"format_version":1,"aead_algorithm":"xchacha20-poly1305","nonce":"MDEyMzQ1Njc4OTo7PD0-P0BBQkNERUZH","ciphertext":"KXtLw6q2Ome3-ygxlSSXJSiRNynqfL5O04B4V2dKH2IZa8LGrcGrA8a2IjClZJx91Wc-oEgHSgpc4cEdRLewapKA2qQ2o7dD4dFNylFv0HwI3Xb1lHgELZudtptc3WHuaYXxh9kUcpHtTkyUR6mG6a9eycbrUwPCskiKwOGlQaeA8RKh0st0bMvGv0V-iodvxMc"}
```

The canonical locator-file bytes are:

```json
{"format_version":1,"aead_algorithm":"xchacha20-poly1305","nonce":"SElKS0xNTk9QUVJTVFVWV1hZWltcXV5f","ciphertext":"21kZgoNEofuzgJGHc6_0lIRa4mrLiIQL-BtJQRlgAOcsbd8-Tm8kRWmNoaYIozjD2ZbFMXi1h4mQH3XJcTb4YQLOirlrDg5EpDRMcNSWd6ap14D7IJBUcs5nKEGyEv_eHjcyYQYGWrgfUNuTULwZcKjuDoo4boTrGlrhH0BpXSuecCxurdY7mAxrqJG0RGu01ROMfIKY5tusH_rj"}
```

None of the three canonical files has a trailing newline. Their byte evidence is:

| File | Length | SHA-256 |
| --- | ---: | --- |
| `lifecycle-key.json` | 109 | `84034a0d4f3f3a5da21f6382cc0c51c8e342b75ad5f89154fde0cf11857e175c` |
| `deployment-record.json` | 312 | `cd35106d11178a0f7866e41a63c69185e4972545661305de1861bcaaa3b9265e` |
| `database-locator-ICEiIyQlJicoKSorLC0uLw.json` | 325 | `fea370d38c5044ea9b42eefb5d4819033387d1c669dc9f31641642125bb9deb2` |

## Test Evidence

`weavelit-server-lifecycle` has direct tests for every startup classification,
the known-answer vector above, strict and bounded key, record, locator, setting,
Base64, and canonical JSON parsing, restrictive and atomic local writes,
rejection of client-supplied paths and file references, Server-derived backend
paths, encrypted connection-secret persistence and restart reopening,
deployment-identifier matching, backend selection and replacement, mutation
serialization, workflow exclusivity, every cross-store crash point, seal
reconciliation, direct invocation after sealing, rejection before secret or
backup reading, redaction, and fail-closed missing, malformed, mismatched,
unavailable, and integrity-failing state.

Real-filesystem tests use isolated roots to exercise missing and malformed
configuration, root execution, every symlink position, wrong owner and mode,
hard links, non-regular and unknown entries, the 256-entry bound, lock
contention, unsupported or failed synchronization, temporary cleanup, key-only
bootstrap recovery, every invalid partial anchor set, and every failure point
before and after file sync, rename, record commit, directory sync, and orphan
cleanup. Tests retain and reopen real SQLite `-journal`, `-wal`, and `-shm`
sidecars through the backend rather than deleting them as lifecycle orphans.

Format and cryptographic negative vectors change one property at a time:
oversized, empty, truncated, invalid UTF-8, non-canonical, duplicate, unknown,
missing, reordered, unsupported-version, malformed Base64, wrong-length,
all-zero disallowed values, wrong key, wrong nonce, wrong associated data,
renamed locator generation, altered ciphertext, and altered tag. Every failure
asserts the exact category/reason pair and proves that sentinel identifiers,
cryptographic values, settings, filenames, paths, sizes, and dependency errors
are absent from diagnostics and client output.

Application Database integration tests verify workflow checkpoint
discrimination, atomic one-time state replacement, and deployment-identifier
enforcement. Server process tests verify route gating and each transition from
restricted pre-operational state to normal operation.

## Related Documents

- [Technical Specification](../../spec.md)
- [Security Model](../../security-model.md)
- [Glossary](../../glossary.md)
- [Lifecycle Anchor Protection And Serialization Profile](lifecycle-anchor-profile-decision.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server Init Design](init/init-design.md)
- [Server Restore Design](restore/restore-design.md)
- [Testing and Validation Policy](../../testing.md)
- [Application Database Design](../database/application-database-design.md)
