# Authentication Design

This document is the canonical destination for implementation-specific
authentication design for the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**.
Binding application requirements remain in the
[Technical Specification](../../spec.md), and approved authentication and
session security profiles remain in the
[Security Model](../../security-model.md). This document owns how the Server
implements those requirements and profiles.

## Scope

This document defines the implementation design for
**[Local Authentication](../../glossary.md#identities-and-access)** credential
handling, sessions, and the compiled-in TOTP
**[MFA Module](../../glossary.md#applications-and-interfaces)**.
**[External Authentication](../../glossary.md#identities-and-access)**
implementation design has not moved here yet.

## Password Hashing

Local Human User passwords are hashed with Argon2id version 1.3 from the
RustCrypto `argon2` crate using `m=65536 KiB`, `t=3`, `p=1`, a 16-byte random
salt, and a 32-byte output, and are stored in PHC string format. This is the
current profile: every new and replacement verifier is produced at it. This
satisfies the
[Security Model](../../security-model.md#password-and-session-security-profile)'s
adaptive password-hashing requirement.

### Accepted Verifier Profiles

A stored verifier is attacker-influenced input. It is read from the
**[Application Database](../../glossary.md#applications-and-interfaces)**, and a
**[Restore](../../glossary.md#states-and-requests)** can install one whose
encoded parameters its author chose. Argon2 verification allocates the memory
the encoded string asks for, so a verifier such as
`$argon2id$v=19$m=4194304,t=100,p=16$…` would make one unauthenticated login
attempt request roughly 4 GiB.

The Server therefore accepts stored verifiers by a closed allowlist rather than
by a bound. A stored verifier is verified against only when its algorithm,
version, memory cost, iteration count, degree of parallelism, decoded salt
length, and output length all exactly match an entry in an explicitly listed set
of accepted profiles. An absent version field, an unknown or extra PHC
parameter, a key identifier, associated data, a different password-hashing
function, and an unparseable string are all outside the list.

Every accepted profile must sit within a 64 MiB verification ceiling, which
bounds what a single unauthenticated login attempt can cost. That invariant is
enforced where the profile set is constructed, so a profile above the ceiling
cannot be configured at all.

The allowlist currently holds exactly one entry: the current profile above.
Adding an entry is a deliberate, reviewed change that accepts stored verifiers
at that profile until it is removed, and it must be recorded here in the same
change.

A verifier outside the allowlist is refused as an authentication failure. It is
never attempted, so its encoded cost parameters never reach the hashing library.

### Rehashing On Profile Drift

After a successful verification against an accepted profile that is not the
current profile, the Server produces a fresh verifier for the submitted password
at the current profile and returns it to the caller to persist in place of the
stored one. A verifier already at the current profile produces no replacement, a
denied authentication never produces one, and a replacement is always produced
at the current profile rather than at the profile the stored verifier used.
Rehashing is therefore reachable only for a profile that the allowlist accepted
and that the ceiling already bounded.

### Denial Without Account Disclosure

An unknown account, an inactive account, an account with no stored verifier, and
an account whose stored verifier is outside the allowlist are all denied only
after one real Argon2 verification against a decoy verifier built at the current
profile. Every denial therefore performs the same verification work as a wrong
password and is indistinguishable from it. The decoy is a valid PHC string with
a random salt and a random output, so no submitted password can match it. A
denial is not reported as an error, so no caller can separate "no such account"
from "wrong password" by inspecting a failure value.

This equal-work property is proved by counting the verification operations a
decision performs through an injected verification seam, not by comparing
elapsed time.

## Session Representation

A session token is an opaque 32-byte random value encoded as unpadded
Base64url. The Application Database stores only the token's SHA-256 hash and
its lifecycle metadata; the plaintext token is never persisted. The Server
issues the session in a `__Host-weavelit_session` cookie with `Secure`,
`HttpOnly`, `SameSite=Strict`, `Path=/`, and no `Domain` attribute.

The session uses a balanced lifetime policy: a 30-minute idle timeout, a
12-hour absolute maximum, and a browser-session cookie carrying no `Max-Age`
or `Expires` attribute.

The stored hash is domain-separated, so a session hash and a CSRF hash of the
same bytes are different values. Neither a token nor a stored hash renders
through `Debug` or `Display`; both produce a fixed redacted string instead.

A stored hash is never compared with an ordinary equality operator. The
Application Database locates a candidate session by indexed digest equality,
which compares stored digests rather than bearer values, and the decision to
accept that row as the presented session is a constant-time comparison of the
two digests.

## Session Storage And Lifetime

A session lives in the Application Database's `SessionStore`, which is a
separate contract from restorable application state. A stored session holds the
session and CSRF digests, the owning account, the issuing
**[Client Module](../../glossary.md#applications-and-interfaces)**, the issue
instant, the last-seen instant, and an absolute expiry instant, and nothing
else. It caches no Group, grant, or other authorization data.

Sessions therefore survive an ordinary Server restart, and they never appear in
normalized state or in a backup. A Restore clears every session inside the same
atomic state replacement that installs the restored state, so session
invalidation cannot be skipped by an interruption between two steps.

The absolute expiry is derived once from the issue instant and is never
extended by activity. The clock is injected, so every boundary is enforced and
tested deterministically. If the clock moves backwards so that the present
instant precedes the issue or last-seen instant, the session is refused before
any lifetime arithmetic runs and its recorded activity is not advanced.

The storage and lifetime rules, including the schema constraints that make a
plaintext value unpersistable and the absolute expiry immutable, are specified
in the
[Application Database Design](../database/application-database-design.md#live-session-storage)
and the
[SQLite Application Database Design](../database/sqlite/sqlite-application-database-design.md#live-session-schema).

## Cross-Site Request Forgery Protection

Each session has a separate random per-session CSRF token of 32 random bytes
encoded the same way; the Server stores only its SHA-256 hash alongside the
session. The two tokens are independent random values, so disclosing the CSRF
token to the browser cannot reveal the session token. The token is exposed to
the browser through a secure, same-site readable cookie and is required in the
`X-Weavelit-CSRF` header on every mutating request, alongside same-origin
validation. The Server rotates the CSRF token on login and on MFA or privilege
elevation.

## Implementation Boundary

`weavelit-server-authentication` owns the profile, the allowlist, the equal-work
password decision, and generation and hashing of the session and CSRF values. It
takes no workspace path dependency, so it cannot reach the transport, the
listener, the Application Database, or a
**[Client Module](../../glossary.md#applications-and-interfaces)**. A caller
supplies the stored credential as an inbound value and persists the replacement
verifier and the token hashes the crate returns. Session persistence and session
lifetime enforcement are owned by the Application Database contract; cookie
emission and route contracts are owned outside both.

The `argon2` and `subtle` dependency records, and the authentication crate's use
of the already-approved `sha2`, `base64`, `getrandom`, and `zeroize`
dependencies, are recorded in the
[Server Architecture Design](../server-architecture-design.md#approved-production-dependencies).

## TOTP Multifactor Authentication

The compiled-in TOTP MFA Module uses the `totp-rs` library and the RFC 6238
profile: HMAC-SHA-1, 6 digits, a 30-second period, and `T0=0`. A secret is a
random 160-bit value stored as unpadded RFC 4648 Base32 and is provisioned
through an `otpauth://` URI disclosed exactly once. Verification accepts the
current time step and one step on either side, and the Server atomically
persists the last accepted step to reject a replay within that window. This
implements the
[Security Model](../../security-model.md#multifactor-authentication-security-profile)'s
enrollment and disclosure requirements.

## MFA Module Enablement

The TOTP Module starts disabled after Init, so the deployment's single Admin
user authenticates with password only until an Administrator explicitly
enables the Module. Enrollment is unavailable before the Module is enabled.
Once TOTP is enabled and a Human User is enrolled, code verification gates
every login for that user. A user who is required to use MFA but is not yet
enrolled receives no application session; the Server fails closed with an
enrollment prompt, and only when the required Module is enabled.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Glossary](../../glossary.md)
