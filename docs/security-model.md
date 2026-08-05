# Weavelit Security Model

This document defines Weavelit's stable security model beneath the
[Technical Specification](spec.md). It owns protected-asset classifications,
trust assumptions, cross-cutting security invariants, and approved security
profiles whose consistency must not depend on incidental implementation
choices. The Technical
Specification owns application behavior and high-level security outcomes;
component design documents own implementation structure, libraries, data
formats, control flow, and error handling.

When Weavelit standardizes a security mechanism across implementations, this
document owns the approved mechanism and minimum security profile. Component
designs explain how their implementation satisfies that profile without
weakening it. This document does not repeat requirements merely to summarize
the Technical Specification or component designs.

## Trust Boundaries And Assumptions

The Server-owned authentication, authorization, lifecycle, cryptographic, and
persistence contracts form the trusted application enforcement boundary.
Compiled-in modules operate within the Server process but remain limited to
their declared responsibilities. A
**[Client Module](glossary.md#applications-and-interfaces)** may authenticate
and translate requests, but it is not an authorization authority.

Client applications, AI agents, agent instructions, caller-supplied identity or
permission claims, request payloads, and imported backups are untrusted. The
Server validates them within the owning Server contract before they can change
trusted state or cause an external side effect. Client validation, route
visibility, and presentation controls improve usability but provide no security
authority.

This model addresses unauthorized or malformed requests, untrusted or
compromised clients and agents, credential disclosure through interfaces,
logging, or persistence, hostile imported data, authorization bypass, and
partial or failed security-sensitive workflows. It does not claim to preserve
application security after a person with sufficient host authority can replace
the Server binary, read protected Server state, or destroy all deployment
anchors.

TLS-protected HTTPS is an application invariant. The deployment operator
supplies and protects the TLS material and controls network exposure, host
access, filesystem protections, and custody of secrets retained outside
Weavelit. A person with sufficient host authority can replace the Server binary
or destroy all persistent deployment anchors. Weavelit cannot distinguish
complete destruction of those anchors from a new installation; preventing and
detecting that action belongs to deployment access control and monitoring.

Host availability, power, installation and deployment execution, environment
validity, and recovery material retained outside Weavelit are deployment
operator responsibilities. The Server's fail-closed response to retained
partial lifecycle state is defined by the [Technical Specification](spec.md#operating-responsibility-and-lifecycle-interruption).
This allocation does not relax any Server validation or protection of untrusted
clients, data, secrets, authentication, or authorization.

## HTTPS Listener And Pre-Operational Surface Security Profile

The direct TLS termination and listener requirements are defined by the
[Technical Specification](spec.md#https-listener-and-pre-operational-exposure).
The host must supply PEM certificate and private-key files that are regular,
non-symlink files protected by restrictive filesystem permissions. The private
key must be readable only by the Server process's effective user or a narrowly
scoped group granted solely to read TLS material, and neither file may be
modifiable by identities that do not administer TLS material. The Server must
validate that the certificate and private key form a usable pair before it
binds its listener and must not expose a route or diagnostic listener when this
validation fails.

The Milestone 1 unauthenticated **[Pre-Operational Surface](glossary.md#applications-and-interfaces)**
is reachable only from IPv4 and IPv6 loopback through the deployment network
boundary; the Server has no allowlist configuration channel. Its status route
enforces the request, connection, handler, handshake, rate, timeout, response,
and parsing bounds defined in the [Web UI Pre-Operational Status Surface](client-modules/web-ui/pre-operational-status-design.md).
It accepts no request body, performs no decompression or cryptographic work, and
does not mutate state. It sends no CORS headers, supports no credentials or
cookies, and has no CSRF flow. The runtime rejects a configured non-loopback
listener address and binds one direct TLS listener only after validated TLS
material and trusted lifecycle classification; it does not create a cleartext
HTTP fallback or an alternate listener. A route-composition, TLS-listener, or
bind failure emits only the fixed `preoperational_unavailable` /
`https_listener_unavailable` startup classification and exits before route
exposure. A later browser-accessible or unauthenticated surface must select
explicit source networks, origin controls, restricted cross-origin resource
sharing (CORS), and cross-site request forgery (CSRF) protection appropriate to
its request model. These controls must not depend on client-side enforcement.

## Protected Assets

The security model protects:

- **[Human User](glossary.md#identities-and-access)** passwords, password
  verifiers, MFA provisioning values, factor data, authentication codes, and
  sessions;
- **[Automation Identity](glossary.md#identities-and-access)** credentials,
  named **[Operation](glossary.md#applications-and-interfaces)** scopes, and
  **[Responsible Owner](glossary.md#identities-and-access)** assignments;
- **[Service Connection](glossary.md#applications-and-interfaces)** credentials
  and other provider authentication material;
- Server-local at-rest keys, backup recovery private keys, unwrapped backup
  keys, and decrypted backup content;
- **[Application Database](glossary.md#applications-and-interfaces)** state and
  the integrity of the deployment record and database locator; and
- the confidentiality and integrity properties of
  **[System Logs](glossary.md#applications-and-interfaces)**,
  **[Audit Logs](glossary.md#applications-and-interfaces)**, and client-visible
  errors.

## Password And Session Security Profile

Local **[Human User](glossary.md#identities-and-access)** passwords must be
stored only as a password verifier produced by Argon2id version 19 with
parameters no weaker than a memory cost of 65536 KiB, a time cost of 3, and a
parallelism degree of 1. Each verifier must use a unique cryptographically
random 16-byte salt and produce a 32-byte output. A deployer-supplied
configuration must not lower this baseline. Passwords must never be stored in
plaintext or reversibly encrypted.

Every password verifier record must carry an explicit version, and Server-owned
authentication logic must reject an unrecognized or non-allowlisted verifier
version before it drives Argon2id resource allocation. Verification must run in
constant time regardless of whether the submitted password is correct, and an
authentication attempt for an unknown Human User must perform one bounded dummy
verification and return the same generic authentication result as a known
Human User's failed attempt. The Authentication Design owns the concrete
verifier record representation, maintained library selection, and
verification control flow that satisfy this profile.

A supported older verifier profile must be upgraded to the current profile only
atomically and only immediately after a successful authentication; the Server
must not rehash following a failed attempt and must not rehash during
**[Restore](glossary.md#states-and-requests)**. **[Init](glossary.md#states-and-requests)**
must create a Human User's first verifier at the current profile, and a
supported older profile carried by a restored Application Database remains
usable and upgrades on that Human User's next successful authentication.

Password creation, change, reset, storage, hashing, and verification must occur
only in Server-owned authentication logic. Client applications and
**[Client Modules](glossary.md#applications-and-interfaces)** may request or
transport these workflows, but they must not persist passwords or password
verifiers or implement independent password hashing or verification.

A session is a random opaque bearer token for which the Server stores only a
verifier and never the token itself. Browser sessions must use secure,
Server-managed session handling. A session's
idle expiry is 30 minutes, its absolute expiry is 12 hours fixed at issuance,
and reauthentication does not extend that absolute expiry. A fresh-authentication
window remains valid for 15 minutes after reauthentication.

The Server must revoke every session belonging to the affected Human User after
a password change or reset, activating or deactivating the Human User's
account, an MFA enrollment, replacement, removal, or reset, an MFA requirement
change, or an **[MFA Module](glossary.md#applications-and-interfaces)**
enablement change affecting an existing enrollment; the existing Restore and
MFA Module-disablement invalidation requirements remain in force. Logout
revokes only the current session, and an
**[Administrator](glossary.md#identities-and-access)** may revoke every session
belonging to another Human User. Reactivating a deactivated Human User must
never revive a prior session, and per-request authorization must make a grant
change effective immediately without relying on a cached grant.

Reauthentication requires the Human User's current password and every
enrolled, enabled MFA factor; a password alone is sufficient only when the
Human User has no enrolled factor, including the first Administrator created
during Init. The Server must require fresh authentication before it performs:
a password change; an Administrator-initiated password reset; an MFA
enrollment, replacement, removal, reset, or requirement change, or a relevant
MFA Module enablement change; activating or deactivating a Human User;
granting or removing the
**[Server Administration Permission](glossary.md#identities-and-access)** or
**[Administrators Group](glossary.md#identities-and-access)** membership; an
**[Automation Identity](glossary.md#identities-and-access)** credential issue,
rotation, or revocation, a scope change, or a
**[Responsible Owner](glossary.md#identities-and-access)** reassignment; and
an Application Database backup creation or download.

This profile approves only the password-verifier, session, revocation, and
reauthentication requirements above. It does not itself approve a new MFA
method, an authorization rule, client presentation or transport,
**[External Authentication](glossary.md#identities-and-access)**, an
Automation Identity credential policy, code, a dependency selection, a
production implementation, or a change to the
[Technical Specification](spec.md).

## Multifactor Authentication Security Profile

The initial **[Time-Based One-Time Password (TOTP)](glossary.md#identities-and-access)**
enrollment flow must confirm both the
**[Human User](glossary.md#identities-and-access)**'s current password and a
current code generated from the new provisioning value. The provisioning value
may be disclosed only to that Human User during enrollment and must never be
returned after enrollment. The Server must exclude TOTP provisioning values,
secrets, and codes before System Logs or Audit Logs reach a Log Module.

A Human User who is required to use MFA but has no valid enrollment must not
receive a usable session until enrollment in an enabled
**[MFA Module](glossary.md#applications-and-interfaces)** is complete.
Resetting an enrollment must immediately invalidate its prior factor data and,
when MFA remains required, force new enrollment before another usable session
is issued.

Before disabling an MFA Module with dependent enrollments, the Server must
report the number of affected Human Users. Disabling the module must immediately
prevent enrollment and verification through that method and terminate every
session belonging to an affected Human User. Disablement must not remove any
Human User's MFA requirement. A Human User who still requires MFA must enroll
through an enabled MFA Module; a Human User without an MFA requirement may
authenticate without MFA and may enroll through any enabled MFA Module.

An Administrator who has enrolled in MFA must complete TOTP verification for
the current session before requiring MFA for a Human User or resetting an MFA
enrollment. The Server must record MFA policy changes and resets without
recording provisioning values, secrets, or codes.

## Secret And Key Material Security Profile

Server-local at-rest key material and the backup recovery key pair must remain
distinct and must not be substituted for one another. At-rest key material
protects reversibly encrypted application values. The recovery public key
protects backup artifacts, and the corresponding private key is accepted only
to decrypt a compatible backup.

The approved Server-local at-rest profile uses one cryptographically random
256-bit key per deployment and XChaCha20-Poly1305 authenticated encryption. Each
encryption must use a fresh cryptographically random 192-bit nonce, retain the
complete 128-bit authentication tag, and bind the product, format version, and
protected artifact kind as associated data. Authentication must succeed before
protected plaintext is parsed or used. The Server must map malformed envelopes,
wrong keys, and authentication failures to the same redacted integrity result.

The Server must run as a non-root operating-system identity. The supported
Debian package must create a locked, non-login `weavelit` system user and primary
group with no supplementary group by default. Runtime enforcement remains
identity-neutral so a dedicated non-root development or container user may own
its state. A later package requirement may grant one narrowly scoped group for
a separately protected host resource, but that group must not receive access to
Server state.

The host must provision the directory named by `WEAVELIT_STATE_ROOT` as a
normalized absolute path whose components are existing non-symlink directories.
The final root must be owned by the Server process's effective user with exactly
mode `0700`. Every managed child must be a regular non-symlink file owned by the
same user, have exactly mode `0600`, and have one hard link. The Server must set
an owner-only umask before creation, operate relative to one validated root
directory handle, and hold an exclusive process-lifetime lock. Package,
service-manager, development, and container configuration may select the state
root but must not supply the key, child filenames, or individual artifact paths.

The root uses a closed code-owned filename inventory. Unknown names, unsafe
entries, or a filesystem that cannot provide required atomic replacement or
advisory locking must fail closed without a reduced-security mode. Failures of
the configured file or directory synchronization operations during a valid run
must also fail closed. These controls preserve active-operation integrity; they
do not promise persistence across host power loss, filesystem loss or
corruption, abrupt process termination, or an operator-broken environment. A
future release may deliberately add code-owned names; older binaries must
reject rather than ignore them.

Milestone 1 does not rotate this key in place and has no external monotonic
anchor. A missing, malformed, corrupted, or wrong key must fail startup closed
when another deployment anchor exists and must never trigger key regeneration.
The profile must detect tampering, interrupted or mixed anchor replacement, and
independent anchor replay. It does not guarantee detection when sufficient host
authority coherently replaces the complete key, deployment record, and database
locator set with an older valid set.

A valid key with no other deployment or database artifact is incomplete
bootstrap state and must fail startup closed; it must not trigger key reuse,
record creation, or another automatic recovery action. Owned key and decrypted
plaintext buffers must be zeroized on normal and error exits where the
maintained facilities can control their storage, without claiming protection
against unavoidable copies, swap, process-memory inspection, or host compromise.

The lifecycle anchor store implements this profile with maintained
XChaCha20-Poly1305, operating-system randomness, strict typed JSON, canonical
Base64url, zeroizing key and plaintext buffers, and safe directory-relative
Unix operations. It authenticates before parsing, never opens the Application
Database while loading anchors, and maps malformed, wrong-key, tampered,
unsupported, unsafe, partial, or unavailable state to payload-free lifecycle
categories.

Lifecycle diagnostics must contain only an approved fixed category and reason
code. They must not include dynamic identifiers, backend or field names,
cryptographic values, plaintext or ciphertext, file facts, paths, sizes, raw
dependency or operating-system errors, or another rejected value. An untrusted
startup state must exit before binding HTTPS rather than expose a diagnostic or
fallback recovery service.

Sensitive authentication material submitted through a
**[Service Connection](glossary.md#applications-and-interfaces)** type's
declared setup workflow must terminate in Server-owned credential handling. The
Server alone stores, uses, refreshes, or revokes that material. It must not
return the material to a client, retain it in a Client Module, or include it in
an Audit Log.

Client applications must transmit initialization secrets, credentials, and
**[Restore](glossary.md#states-and-requests)** private recovery keys over HTTPS.
They must not log them, place them in URLs or browser history, or retain them in
client-side persistent storage. A Restore-capable client may read a selected
encrypted backup only to transmit it and must not retain another copy.
**[Init](glossary.md#states-and-requests)**-capable and Restore-capable Client
Modules must pass sensitive inputs only to their corresponding Server-owned
contracts and must not decrypt, log, or retain them.

The Server must never return a submitted secret and may persist it only in its
intended protected representation, such as a password verifier or encrypted
credential. The backup recovery private key must not be persisted in the
Application Database, Server configuration, a container volume, logs, or an
ordinary backup artifact. Possession of that private key authorizes decryption
of its compatible backup only; it is not an application identity, proof of host
authority, or authorization for another function.

During pre-operational Application Database selection, a client may submit
backend-declared connection values but must not submit or influence a filesystem
path or file reference. The Server must derive every local path, persist secret
connection values in encrypted form within protected Server-owned configuration,
and decrypt them only when required to open the selected database. A
client-supplied value must never cause the Server to read an unrelated local
file as connection material.

## Recovery Key Security Profile

This profile defines the currently approved backup recovery-key cryptographic
material, delivery, and authenticity properties introduced by the Secret And
Key Material Security Profile above. The accepted
[Recovery Key Profile Decision](server/lifecycle/recovery-key-profile-decision.md)
preserves the rejected alternatives and reversal boundary for this choice; this
section is the current-policy authority.

The approved recovery-key profile is HPKE (RFC 9180) base mode with
DHKEM(X25519, HKDF-SHA-256) (KEM `0x0020`), HKDF-SHA-256 (KDF `0x0001`), and
ChaCha20-Poly1305 (AEAD `0x0003`). A version 1 public recovery-key document is
structured JSON carrying a public kind, a fixed profile identifier, format
version `1`, and an RFC 8037 X25519 public JWK. A version 1 private document
carries the matching fixed fields, a private kind, and the JWK `d` value, and
must include or match the public `x` value where required for validation. Both
`x` and `d` must be canonical unpadded Base64url and must decode to exactly 32
bytes. Key-document processing must bound input size, use structured parsing,
and reject duplicate, unknown, missing, unsupported, mismatched `d`/`x`,
wrong-length, noncanonical, low-order, and trailing-content input.

**[Init](glossary.md#states-and-requests)** delivers the private document
exactly once as a compact UTF-8 copyable text artifact. The Server never
redisplays, logs, stages, backs up, or persists it; safeguarding it outside
Weavelit remains the responsibility of the person completing Init. The Init
proof mechanism that demonstrates possession of this key without persisting it
is defined in the [Server Init Design](server/lifecycle/init/init-design.md).

Every backup wraps a fresh 32-byte data-encryption key to the retained recovery
public key using HPKE, with `info` and authenticated associated data derived
from the complete canonical security header, and the recovery-key identifier
appears only inside authenticated ciphertext rather than cleartext header
metadata. This profile's approved authenticity property is AEAD integrity and
confidentiality only: it detects tampering but does not prove Weavelit Server
origin, because no origin-signing key is selected for this profile. A
component must not state or imply Server-origin authenticity for a decrypted
backup. The [Server Restore Design](server/lifecycle/restore/restore-design.md)
and [Application Database Design](server/database/application-database-design.md)
own the concrete wrapping, header, and validation mechanics that satisfy this
profile.

Milestone 1 has no recovery-key rotation workflow. **[Restore](glossary.md#states-and-requests)**
preserves the existing authenticated recovery public key, and a compatible
existing or future backup uses the same externally retained private key. This
is a deliberately limited current scope rather than a permanent ban; rotation
requires a later custody, old-backup, and migration decision recorded in
[Open Questions](open-questions.md). The recovery key is never an application
identity, host-authority proof, authorization grant, or at-rest key.

## Backup Input Security Profile

The Server must treat every submitted backup as untrusted even when the supplied
recovery key can decrypt it. Before application-state mutation, Restore must
bound upload size, cryptographic work, decompression, parsed structures,
collections, strings, execution time, and concurrency. It must validate the
backup's authenticity, integrity, format version, compatibility, references,
and complete contents. A rejected backup must leave the selected database
without application state and expose only stable, redacted errors.

Restore must not persist a private recovery key, unwrapped backup key, or
decrypted backup plaintext. If temporary staging is required, the Server may
persist only the bounded encrypted artifact in protected storage and must remove
it after success or rejection before lifecycle interruption. Retained staging
state after interruption must remain fail-closed without Server-managed cleanup
or resumption. The Restore Design owns concrete formats, algorithms, bounds,
validation order, staging mechanics, and cleanup flow.

## Log And Client Output Security Profile

The Server must remove secrets and unnecessary sensitive payload data before a
System Log or Audit Log reaches a Log Module. Log Modules receive already
redacted records and must not become an alternate sanitization boundary.

System Logs, Audit Logs, and client-visible errors must not contain passwords,
password verifiers, MFA provisioning values or codes, Service Connection
credentials, private or unwrapped keys, decrypted backup content, or other
submitted secrets. Client-visible errors must also exclude raw internal traces,
dependency-specific details, and sensitive filesystem information. Redaction
must preserve the accountability fields required for an Audit Log and the
diagnostic classification required for a System Log.

Log-record validation failures must use stable payload-free errors. Their
display and diagnostic representations must not include rejected values, and a
logging-required workflow must fail closed when a pre-redacted bounded record
cannot be constructed.

## Component Security Ownership

- The [Authentication Design](server/authentication/authentication-design.md)
  owns password, session, local MFA policy, and external-authentication
  implementation.
- MFA Module designs own method-specific enrollment, verification, and
  protected factor-data implementation; the Server retains policy,
  authorization, session, recovery, and audit authority.
- The [Authorization Design](server/authorization/authorization-design.md) owns
  grant evaluation, access-class enforcement, disablement precedence, and
  per-request authorization implementation.
- The [Automation Identity Design](server/automation-identities/automation-identity-design.md)
  owns credential lifecycle, Responsible Owner enforcement, scope mechanics,
  and accountability integration.
- Server Audit design owns Audit Log construction and pre-redaction, Server
  Observability design owns System Log construction and pre-redaction, and
  Server API design owns stable client-error presentation and redaction. Log
  Module designs own processing of records only after the Server completes the
  pre-redaction boundary.
- The [Server Lifecycle Design](server/lifecycle/lifecycle-design.md),
  [Server Init Design](server/lifecycle/init/init-design.md), and
  [Server Restore Design](server/lifecycle/restore/restore-design.md) own
  deployment-state integrity, sensitive workflow handling, and fail-closed
  lifecycle implementation.

Every component design and implementation must preserve the trust boundaries
and security profiles in this document. A component may strengthen a local
control but must not weaken, bypass, or reinterpret these invariants.

Security-sensitive behavior changes must include automated evidence under the
[Testing and Validation Policy](testing.md). Evidence must exercise the
applicable denial path, secret non-disclosure and non-persistence properties,
redaction boundary, and fail-closed behavior rather than testing only successful
requests.

## Related Documents

- [Technical Specification](spec.md)
- [Glossary](glossary.md)
- [Lifecycle Anchor Protection And Serialization Profile](server/lifecycle/lifecycle-anchor-profile-decision.md)
- [Recovery Key Profile Decision](server/lifecycle/recovery-key-profile-decision.md)
- [Authentication Design](server/authentication/authentication-design.md)
- [Authorization Design](server/authorization/authorization-design.md)
- [Automation Identity Design](server/automation-identities/automation-identity-design.md)
- [Server Lifecycle Design](server/lifecycle/lifecycle-design.md)
- [Server Init Design](server/lifecycle/init/init-design.md)
- [Server Restore Design](server/lifecycle/restore/restore-design.md)
- [Testing and Validation Policy](testing.md)
