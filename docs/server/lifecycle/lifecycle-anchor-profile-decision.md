# Lifecycle Anchor Protection And Serialization Profile

## Status

Accepted.

## Context

The shared **[Weavelit Server](../../glossary.md#applications-and-interfaces)**
lifecycle must persist a deployment record and an
**[Application Database](../../glossary.md#applications-and-interfaces)**
locator before normal application state exists. The record is security-critical
even when it contains no secret, and the locator may contain secret connection
values. Both files must survive process and host restarts, reject tampering and
partial replacement, and remain portable across the supported package,
development, and future container environments.

The existing host trust model assumes that the deployment operator protects
the Server process and its persistent state. It does not claim to resist a
person with sufficient authority to replace the Server binary, read protected
Server state, or coherently replace every persistent deployment anchor. The
selected profile therefore strengthens the file-level integrity boundary
without introducing a Trusted Platform Module (TPM), external key-management
service, or remote monotonic authority into Milestone 1.

## Decision

### Trusted State Root

The Server requires one non-secret `WEAVELIT_STATE_ROOT` environment variable.
It names an absolute, normalized, existing directory provisioned by the host.
Every path component must be a real directory rather than a symbolic link. The
Server has no fallback state location and does not accept a state root, child
path, filename, or file reference from a client.

The Server runs as a non-root operating-system identity. The final root is
owned by that effective user with exact mode `0700`; every managed child is a
regular one-link file owned by the same user with exact mode `0600`. The
supported Debian package creates the locked, non-login `weavelit` system user
and primary group with no supplementary group by default, while runtime checks
remain identity-neutral for development and container profiles.

The lifecycle crate opens the root once, operates relative to its directory
handle, and holds an exclusive non-blocking process lock for the Server
lifetime. Version 1 uses a closed code-owned filename inventory, including the
SQLite database and its recovery sidecars. Unknown, unsafe, or excess entries
fail startup. Future versions may deliberately expand the inventory; an older
binary encountering those names fails closed.

### Key Custody And Rollback Boundary

Each deployment has one cryptographically random 256-bit Server-local at-rest
key in a code-named file beneath the trusted state root. The key file is created
without following symbolic links, is owned by the effective service user, and
uses mode `0600`. The at-rest key is distinct from every backup recovery key and
is never supplied by a client, stored in the Application Database, or written
to logs or client output.

Milestone 1 does not rotate the at-rest key in place. If the key is missing,
malformed, corrupted, or does not authenticate an existing anchor, startup
fails closed and the Server never generates a replacement key for that retained
state. Recovery from permanent key loss requires a new deployment and a valid
encrypted backup with its separate recovery private key.

A valid key by itself is the only resumable incomplete bootstrap state. When no
record, locator, Application Database file, or SQLite sidecar exists, startup
reuses that key and creates a fresh deployment identifier and record. Every
other partial bootstrap combination fails closed.

The profile detects malformed or tampered anchors, interrupted replacement,
deployment-identifier mismatch, and independently replayed or mixed record and
locator generations. It does not detect coherent replacement of the complete
key, record, and locator set with an older valid set by sufficient host
authority. Adding that guarantee requires a separately trusted monotonic anchor
and is outside Milestone 1.

### Serialization And Authenticated Encryption

The key file, deployment-record envelope and payload, and database-locator
envelope and payload use numerically bounded, versioned, compact canonical UTF-8
JSON. Binary values use canonical unpadded URL-safe Base64. Readers reject
duplicate, unknown, missing, reordered, or non-canonical fields and bytes,
unknown enum values, trailing content, and unsupported versions. The exact
version 1 schemas, limits, associated data, filenames, and public known-answer
vector are authoritative in the [Server Lifecycle Design](lifecycle-design.md).

A version 1 reader accepts only version 1. A future release explicitly lists
and implements every older version it can migrate; the profile makes no
standing promise to read the immediately previous or every historical version.
An older binary never ignores state whose security meaning it does not
understand.

The deployment record and locator are each protected as a complete payload with
XChaCha20-Poly1305 authenticated encryption. Each write uses the deployment key,
a fresh cryptographically random 192-bit nonce, the complete 128-bit tag, and
associated data that binds the Weavelit product, format version, and artifact
kind. The envelope identifies the format and algorithm but exposes no protected
payload field. Authentication succeeds before payload parsing, and every
authentication failure maps to the same redacted integrity result.

XChaCha20-Poly1305 is selected for its modern authenticated-encryption security
and its extended nonce, which permits independently generated random nonces
without a crash-sensitive persistent counter. The files are private Server
artifacts and require no external format interoperability. Weavelit has no FIPS
140-3 requirement; selecting an AES algorithm in an unvalidated Rust library
would not itself make the Server FIPS validated.

The maintained RustCrypto `chacha20poly1305` implementation supplies the AEAD
primitive. Maintained randomness, zeroization, strict JSON, Base64, and safe
Unix filesystem dependencies supply operating-system randomness,
sensitive-buffer clearing, structured parsing, canonical binary encoding, and
race-resistant relative file operations. The pinned Rust standard library
supplies file locking. The Server does not implement cryptographic primitives,
parse these formats with ad hoc string handling, or add an unnecessary locking
dependency.

### Atomic Commit And Diagnostics

Every lifecycle file uses exclusive same-directory temporary creation, complete
write, file synchronization, atomic rename, and state-root directory
synchronization. Database locator files are immutable generations. A new
locator is durably prepared first; atomic replacement of the deployment record's
generation pointer is the commit point. This makes a pre-commit locator an
ignorable orphan and avoids a fixed-locator crash window that could destroy the
previous selection.

The state-root filesystem must support the required atomic replacement,
synchronization, and advisory-lock semantics. The Server has no reduced-
durability mode. Recognized lifecycle temporary files and unreferenced locator
generations are removed and the root is synchronized before routes are exposed;
SQLite recovery sidecars remain under SQLite ownership.

An untrusted startup state emits one compact diagnostic containing only a fixed
category and safe reason code, exits with status `1`, and never binds HTTPS.
Diagnostics carry no dynamic values. The exact taxonomy is authoritative in the
Server Lifecycle Design.

## Rejected Alternatives

- A TPM-, operating-system-, or key-management-service-backed key was rejected
  for Milestone 1 because it would introduce platform-specific provisioning,
  recovery, package, and container contracts beyond the current host trust
  model.
- An external monotonic counter was rejected because complete-set replay by
  sufficient host authority is outside the current threat model. Introducing
  one later changes deployment and recovery requirements.
- A plaintext authenticated deployment record with selectively encrypted
  locator fields was rejected because it requires a second message-
  authentication construction, canonicalization rules, key separation, and
  field-level confidentiality policy. Whole-file authenticated encryption gives
  both artifacts one smaller security boundary.
- AES-GCM was not selected because these private files do not require NIST or
  FIPS compatibility and XChaCha20-Poly1305's random 192-bit nonce is a better
  fit for independent crash-safe file replacement. A future FIPS requirement
  would require a validated cryptographic-provider design, not only an
  algorithm substitution.
- CBOR and a custom binary format were rejected because these small anchors gain
  no material size benefit and strict JSON has a mature parser ecosystem and
  clearer test fixtures.
- A fixed mutable locator filename was rejected because replacement spans two
  files and a crash can otherwise destroy the last committed selection.
  Immutable locator generations and the deployment-record pointer provide one
  explicit commit point without adding a manifest anchor.
- A separate manifest was rejected because it adds another persistent anchor
  and startup state while duplicating the deployment record's authority.
- Concurrent Server processes sharing one state root were rejected because
  serialized file writes cannot keep runtime route state and one SQLite
  connection owner coherent. A lifetime lock makes the ownership boundary
  explicit.
- Permissive or extensible-by-prefix root contents were rejected because an
  unrecognized artifact can affect trusted state or conceal an incomplete
  upgrade. Future releases must add each allowed name or pattern deliberately.
- Ignoring unknown fields or versions was rejected because an older Server must
  fail closed rather than accept state with security semantics it does not
  understand.
- In-place key rotation was rejected for Milestone 1 because it requires a
  separately recoverable multi-store re-encryption protocol across every
  protected value.

## Consequences

The host must provision and preserve one protected state root, including its
key file, deployment record, locator, and Server-managed database files.
Filesystem custody protects the plaintext at-rest key; whole-file authenticated
encryption protects record and locator confidentiality and integrity but does
not protect state from a host authority that can read the key.

Operators must run exactly one non-root Server process for a state root on a
local filesystem with the required durability semantics. Package installation
owns creation of the dedicated service identity and root; it does not create an
application user or complete Init. Adding a host key store later changes service
identity access, provisioning, recovery, and file-format migration rather than
only replacing one library call.

Operators cannot inspect protected record or locator payloads directly. The
Server must provide stable redacted diagnostics and tests rather than treating
manual file inspection or editing as a supported recovery mechanism. Unknown
versions, key loss, authentication failure, and unsafe filesystem state stop
startup without exposing Init, Restore, or normal operation.

The format envelope records its algorithm and version so a future release can
add an explicit migration or validated-provider profile. Such a release must
name the versions it accepts, atomically rewrite every affected anchor and
protected application value, and retain the fail-closed rollback boundary. A
future external monotonic anchor additionally changes commit, Restore,
snapshot, and disaster-recovery procedures because the external value must
advance coherently with local state.

## Related Documents

- [Server Lifecycle Design](lifecycle-design.md)
- [Security Model](../../security-model.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Technical Specification](../../spec.md)
- [Testing and Validation Policy](../../testing.md)
