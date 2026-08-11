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
handling, sessions, the login, session-validation, and logout route layer that
admits and decides those requests, and the compiled-in TOTP
**[MFA Module](../../glossary.md#applications-and-interfaces)**.
**[External Authentication](../../glossary.md#identities-and-access)**
implementation design has not moved here yet. The version 1 route paths,
stable error codes, and result-envelope shape are owned by the
[Server API Contract](../api/api-contract-design.md); this document owns the
decisions behind those routes.

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

The route layer preserves this indistinguishability at the wire. An unknown
account, an inactive account, an account with no stored verifier, an account
whose stored verifier is outside the allowlist, and a wrong password all
produce the identical `401` response: the same stable error code, the same
response body shape, the same absence of any cookie, and no response header
that varies by cause.

## Login Admission And Verification Concurrency

Argon2 verification at the current profile reserves that profile's memory cost
for the duration of one verification, and the listener otherwise admits up to
15 concurrent connections. Running every admitted connection's login
verification at once would let the route reserve that memory many times over,
so the Server instead declares a single-permit admission lane for the login
route: at most one Argon2 verification runs at any time, and the route's peak
reserved verification memory is therefore one profile's cost rather than a
multiple of the listener's connection allowance. This is a deliberate memory
bound, not an incidental limit.

The permit is acquired by the listener's admission stage, which runs before the
request body is allocated, so a login request that cannot be admitted is
rejected before its body exists rather than after it has already been read.
The acquired permit travels with the request into the blocking task that
performs the Argon2 verification and the session-issuance database work that
follows a successful one, and is released only once that work has finished.

The bound has a throughput consequence the Server does not hide: concurrent
login attempts are serialized rather than parallelized, so a burst of
simultaneous logins is verified one at a time and each waits for the
verifications already admitted ahead of it. No other route shares this lane or
is affected by it; only the login route's own concurrency is bounded this way.

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

The session cookie carries no `Max-Age` or `Expires` attribute, so it is a
browser-session cookie rather than a persistently stored one. The only place
an expiry attribute appears anywhere in the cookie contract is the logout
response, which deletes both cookies with `Max-Age=0`.

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
token to the browser cannot reveal the session token. The token is issued in a
`__Host-weavelit_csrf` cookie with `Secure`, `SameSite=Strict`, and `Path=/`,
and no `Domain`, `Max-Age`, or `Expires` attribute. It is deliberately not
`HttpOnly`, unlike the session cookie: the
client application must be able to read it to echo it in the
`X-Weavelit-CSRF` header on every mutating request, alongside same-origin
validation. Reading it discloses nothing the cookie does not already carry,
because it is never a bearer credential on its own without the `HttpOnly`
session cookie it accompanies. The Server rotates the CSRF token on login and
on MFA or privilege elevation.

## Cookie Emission

A response may carry only one of two closed, fixed cookie effects: issuing a
session together with its paired CSRF token, or clearing both. There is no
variant that sets a cookie the two effects above do not already describe, and
no variant that sets one of the pair without the other. Login is the only
response that issues a session, logout is the only response that clears one,
and every other route, including session validation, emits no cookie at all.

Each effect renders to exactly two `Set-Cookie` lines bounded at 512 aggregate
bytes. An effect that would exceed either bound is not truncated: the response
is replaced with the fixed unavailable failure and no cookie is emitted, so a
future attribute change that grew the rendered text fails closed instead of
shipping a partial `Set-Cookie` line.

## Authentication-Failure System Log Recording

A denied local authentication attempt is recorded as a System Log record
before the denial is returned to the caller. The record carries a fresh
Server-generated record identifier, the event time, and the same correlation
identifier the denial response reports, so the two can be related without the
record naming anything the caller submitted. Its classification and detail are
fixed compile-time constants rather than derived from the request, so the
record contains no username, account identifier, password, token, or other
client-supplied text, and an unknown account, an inactive account, a verifier
outside the allowlist, and a wrong password all produce the identical record.

Recording is attempted, not guaranteed: an unconfigured System Log
destination, a clock or randomness failure, and a delivery failure are all
absorbed silently and leave the denial exactly as it would have been without
recording. Delivery therefore can neither enrich, delay, nor change what the
caller receives. The record's fixed classification, detail, and field binding
are owned by the
[Authentication-Failure System Log Record](../observability/authentication-failure-record-design.md).

## Implementation Boundary

`weavelit-server-authentication` owns the profile, the allowlist, the equal-work
password decision, and generation and hashing of the session and CSRF values. It
takes no workspace path dependency, so it cannot reach the transport, the
listener, the Application Database, or a
**[Client Module](../../glossary.md#applications-and-interfaces)**. A caller
supplies the stored credential as an inbound value and persists the replacement
verifier and the token hashes the crate returns. Session persistence and session
lifetime enforcement are owned by the Application Database contract. The login,
session-validation, and logout route decisions, the single-permit login
admission lane, and authentication-failure System Log recording are owned by
the Server executable crate (`weavelit-server`); the shared route contract,
every header and cookie precondition, and the closed cookie effect are owned by
the shared Client Module contract crate (`weavelit-module-client`).

The `argon2` and `subtle` dependency records, and the authentication crate's use
of the already-approved `sha2`, `base64`, `getrandom`, and `zeroize`
dependencies, are recorded in the
[Server Architecture Design](../server-architecture-design.md#approved-production-dependencies).

## TOTP Multifactor Authentication

The compiled-in TOTP MFA Module uses the `totp-rs` library and the RFC 6238
profile: HMAC-SHA-1, 6 digits, a 30-second period, and `T0=0`. A secret is a
random 160-bit value stored as unpadded RFC 4648 Base32 and is provisioned
through an `otpauth://` URI disclosed exactly once. The Server supplies the
twenty secret bytes from the operating-system random source; the Module never
generates them. The Module holds the secret and the provisioning URI in
zeroizing types that redact in `Debug`, so neither can reach a log, an error,
or a response body except through an explicit disclosure. Verification accepts
the current time step and one step on either side. The Module derives and
compares codes only; it reads no clock, takes the verification time as a
parameter, and owns no policy, session, recovery, or audit behavior. This
implements the
[Security Model](../../security-model.md#multifactor-authentication-security-profile)'s
enrollment and disclosure requirements.

Provisioning data is generated exactly once, when an enrollment is opened, and
is returned in that one response only; the Server never returns it again for
the same or a later enrollment. It is also never stored in a retrievable form:
the secret and its `otpauth://` URI exist only in memory, held by the
continuation ticket described below, until a code proves the caller holds
them. Only then is the factor written, and what is written is the sealed
factor data produced by the Server's protected-value authority, not the secret
in any form the Server itself could read back and disclose again. An abandoned
enrollment — one whose ticket expires or is never confirmed — therefore leaves
no factor behind.

### Second-Factor Admission

A login's admission decision is exactly three inputs, evaluated only after the
password has verified: whether the TOTP Module is enabled for the deployment,
whether the account already holds an enrolled factor for it, and whether the
account is required to present one. Every combination of the three resolves to
one of four outcomes:

| Module enabled | Enrolled | Required | Outcome |
| --- | --- | --- | --- |
| Yes | Yes | Yes | Second factor required |
| Yes | Yes | No | Second factor required |
| Yes | No | Yes | Enrollment required |
| Yes | No | No | Session established |
| No | Yes | Yes | Denied |
| No | Yes | No | Session established |
| No | No | Yes | Denied |
| No | No | No | Session established |

An enrolled account that is required to use MFA is denied whenever the Module
is disabled, even though its password verified. This is deliberate: the
deployment stated that the account must present a second factor, and admitting
it because the Module currently cannot verify one would silently drop that
requirement rather than enforce it. The denial is byte-identical to a
wrong-password denial — the same stable error code, the same response body
shape, the same absence of any cookie, and no response header that varies by
cause — so a disabled Module cannot be detected by comparing its response with
an ordinary wrong-password denial. Every remaining row issues a session
directly: MFA gates a login only when the Module is enabled and the account
already holds a factor, or when the account is required to hold one; an
unenrolled, not-required account and an enrolled-but-not-required account whose
Module is currently disabled both proceed straight to a session.

### Password Verification

Every login path costs exactly one password verification: an unknown username,
a wrong password, an accepted login with no second factor, an accepted
password that stops at an enrolled factor, and an accepted password that stops
at enrollment all perform the same single call described in
[Denial Without Account Disclosure](#denial-without-account-disclosure).
Self-enrollment from a live session re-verifies the account's current password
through that same call, so it also costs exactly one verification and takes
the login route's single-permit admission lane.

The routes that follow a continuation — submitting a second-factor code and
confirming an opened enrollment with a code — perform no password verification
and do not take that lane. A code costs one decryption of the sealed factor
data and one HMAC comparison, not an Argon2 verification at the approved
memory cost, so admitting it into the password lane would let a password
verification already holding the single permit block a code that reserves none
of that memory.

### Continuation Ticket

A verified password that must still present a second factor, or must still
enroll one, does not receive a session directly. It receives a continuation
instead: an opaque bearer value that is independently random rather than
derived from the session, the account, or anything else, so nothing about the
account can be recovered from it. The Server retains only its digest, never the
continuation itself, and compares a submitted value against that digest in
constant time.

A continuation is single-use and short-lived. Claiming it removes it from the
Server's store before the submitted code is examined, so one continuation
admits exactly one attempt whether or not that attempt turns out to be correct;
an invalid code still consumes the continuation it was presented with. This is
deliberate: the user must sign in again to obtain a fresh continuation and a
fresh attempt, rather than being allowed unlimited retries against one verified
password. A continuation also expires five minutes after it is issued, so a
verified password that is never followed up cannot be resumed indefinitely.

### Enrollment

Enrollment is opened one of two ways, and each proves current possession of the
account's password by a different means.

On the login-continuation path, a login stopped at `mfa_enrollment_required`
because the account is required to use MFA but holds no factor yet. Opening the
enrollment consumes that continuation, and the continuation is itself the proof
of a current password: it was issued only after this Server verified one. No
password is re-entered on this path.

The self-enrollment path instead opens an enrollment for an account that
already holds a live, unenrolled session — enrolling an optional factor by
choice rather than being required to. Because no continuation exists to prove a
current password, this route requires the live session, that session's
cross-site request forgery token, and the account's current password,
re-verified through the same single call every login uses. Enrolling a factor
from a stolen session still costs the account's password.

Both paths disclose a fresh secret and provisioning URI and issue a second,
separate continuation that confirms the enrollment once a code from that secret
is presented. Confirming binds exactly the disclosed secret: the factor and the
replay watermark that already consumed the confirming code are written
together in one operation, so the enrollment cannot be reopened and confirmed a
second time and the confirming code cannot be presented again.

### Replay Watermark

An acceptance window that spans three time steps would otherwise let a code
observed in transit be presented again while it remains inside that window. The
Server therefore records, per enrolled factor, the highest time step it has
ever accepted. A presented code is accepted only when the step it matched is
strictly greater than that recorded watermark; a matched step at or below the
watermark is a replay and is refused even though the code is arithmetically
correct.

The check and the update are one operation. The Application Database contract
exposes a backend-neutral MFA store that performs the comparison and the write
inside a single transaction, so no concurrent presentation of the same code can
observe the pre-update watermark and be accepted alongside the first. The
decision belongs to the store rather than to a caller precisely because a
caller that read the watermark, decided, and then wrote it would reopen that
window.

A watermark is live operational state, not restorable state. It records what a
factor did in this running deployment, whereas an enrolled factor is part of
the restorable aggregate. It is therefore stored beside the live session state
rather than on the factor record, and a Restore clears every watermark within
the same atomic state replacement that clears every live session. Carrying a
watermark across a Restore would judge a newly presented code against a history
that belongs to a different deployment state.

Because a confirmed enrollment writes the factor and the watermark that
consumed its confirming code in that same operation, the code that confirms an
enrollment cannot then satisfy a login: presenting it again inside its own time
step is a replay against the watermark the confirmation itself just set.

## MFA Module Enablement

The TOTP Module starts disabled after Init, so the deployment's single Admin
user authenticates with password only until an Administrator explicitly
enables the Module. Enrollment is unavailable before the Module is enabled.
Once TOTP is enabled and a Human User is enrolled, code verification gates
every login for that user. A user who is required to use MFA but is not yet
enrolled receives no application session; the Server fails closed with an
enrollment prompt, and only when the required Module is enabled.

Changing enablement is a preview-then-act decision, implemented as
`AuthenticationRuntime::enrolled_accounts` and
`AuthenticationRuntime::set_module_enabled`. An Administrator first previews how
many accounts currently hold a TOTP factor; disabling the Module then names
that same previewed count. The count, the enabled-state write, and the session
revocation described below are one atomic operation: the count is recomputed
and checked against the previewed value inside that same operation, and a count
that no longer matches writes nothing and reports the current count instead, so
a concurrent enrollment cannot change what an Administrator's decision actually
disables. This satisfies the
[Security Model](../../security-model.md#multifactor-authentication-security-profile)'s
requirement to report the number of affected Human Users before disabling a
Module with dependent enrollments.

Disabling the Module also revokes the live session of every account holding a
TOTP factor, in that same atomic operation, because those sessions were
established behind a factor this deployment is no longer willing to verify.
Enabling the Module revokes no session. Disablement never removes an account's
MFA requirement: a required account that holds no verifiable factor is denied
under the [Second-Factor Admission](#second-factor-admission) table above
rather than admitted without one.

These two operations are Server-core primitives; no Administration Plane route
composes them yet, so the Security Model's enablement requirement is satisfied
at the runtime layer and remains to be exposed through an administration
route.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server API Contract](../api/api-contract-design.md)
- [Authentication-Failure System Log Record](../observability/authentication-failure-record-design.md)
- [Glossary](../../glossary.md)
