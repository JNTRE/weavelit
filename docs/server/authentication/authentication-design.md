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

Because that refusal is silent by design, a **[Restore](../../glossary.md#states-and-requests)**
that installed an off-allowlist verifier would produce a deployment whose
accounts can never authenticate. Restore therefore resolves every verifier a
backup carries against this same allowlist before it constructs restored state,
as defined by the
[Server Restore Design](../lifecycle/restore/restore-design.md#backup-validation-and-restored-state).
The allowlist is the single authority for both decisions; neither restates it.

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
password and is indistinguishable from it. An expired temporary credential is
also denied only after its retained verifier receives that same verification
work; expiry is evaluated after the verifier result, never as an early return.
The decoy is a valid PHC string with a random salt and a random output, so no
submitted password can match it. A denial is not reported as an error, so no
caller can separate "no such account" from "wrong password" by inspecting a
failure value.

An account with no stored verifier is a modeled credential state rather than an
error or a data defect. The password decision represents it explicitly, denies
it like any other cause, and neither the decision nor any state-producing
operation treats a missing verifier as invalid: an account may be intentionally
passwordless, disabled, or still pending enrollment. **[Restore](../../glossary.md#states-and-requests)**
therefore validates the verifiers a backup supplies without requiring any
account to have one, as the
[Server Restore Design](../lifecycle/restore/restore-design.md#backup-validation-and-restored-state)
records.

This equal-work property is proved by counting the verification operations a
decision performs through an injected verification seam, not by comparing
elapsed time.

The route layer preserves this indistinguishability at the wire. An unknown
account, an inactive account, an account with no stored verifier, an account
whose stored verifier is outside the allowlist, an expired temporary
credential, a stale verified credential revision, and a wrong password all
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

A listener response deadline does not cancel admitted blocking authentication
work. A direct session login, second-factor verification, or enrollment
confirmation may therefore finish its transaction and issue a session after the
listener returned its stable `gateway_timeout` response or the browser lost the
response. Only a valid success response or a later authenticated session proves
that outcome; a negative session observation at one instant proves neither
refusal nor that the transaction will not commit later. The [Web UI Application
Design](../../clients/web-ui/web-ui-application-design.md#second-factor-steps)
owns the bounded client reconciliation that applies this residual.

## Session Representation

A session token is an opaque 32-byte random value encoded as unpadded
Base64url. The Application Database stores only the token's SHA-256 hash and
its lifecycle metadata; the plaintext token is never persisted. The Server
issues the session in a `__Host-weavelit_session` cookie with `Secure`,
`HttpOnly`, `SameSite=Strict`, `Path=/`, and no `Domain` attribute.

The raw entropy that produces session, CSRF, and continuation bearer values is
filled directly into zeroizing storage and remains there through Base64url
encoding. The encoded bearer itself is also held in a clearing owner; the Server
does not leave an application-owned ordinary random-byte array after encoding.

The session uses a balanced lifetime policy: a 30-minute idle timeout, a
12-hour absolute maximum, and a browser-session cookie carrying no `Max-Age`
or `Expires` attribute.

The stored hash is domain-separated, so a session hash and a CSRF hash of the
same bytes are different values. Neither a token nor a stored hash renders
through `Debug` or `Display`; both produce a fixed redacted string instead.

After successful validation, the Server's internal `ValidatedSession` retains
that stored session hash with the account and issuing Client Module so a
current-session authorization proof can bind to the exact session. This value
is not the bearer token, is not added to the Client Module's session-identity
response, and remains limited to constant-time comparison and fixed redacted
diagnostics.

The session cookie carries no `Max-Age` or `Expires` attribute, so it is a
browser-session cookie rather than a persistently stored one. The only place
an expiry attribute appears anywhere in the cookie contract is the logout
response, which deletes both cookies with `Max-Age=0`.

A stored hash is never compared with an ordinary equality operator. The
Application Database locates a candidate session by indexed digest equality,
which compares stored digests rather than bearer values, and the decision to
accept that row as the presented session is a constant-time comparison of the
two digests.

## Account Credential Issuance Writers

The implemented account creation and password reset routes drive the
transport-independent writers that own temporary credential issuance. Ordinary
Account authorization and a separate exact-session credential-issuance
assurance are both required before a workflow returns a temporary-password
disclosure. The
[Server API Contract](../api/api-contract-design.md#account-credential-issuance)
owns the route, body, result, and typed-error shapes.

The authentication crate prepares each temporary credential from exactly 18
bytes (144 bits) of operating-system randomness encoded as exactly 24 unpadded
Base64url characters. It accepts no caller-supplied entropy and has no fallback
for unavailable or unusable randomness. The plaintext remains in zeroizing
storage, and the prepared value pairs its approved-profile verifier with a
non-clonable disclosure whose only plaintext transfer consumes it. The bundle
is completed before a state mutation begins; only its verifier is
eligible for persistence. The crate exports the fixed 24-hour lifetime as a
typed duration but does not own an issuance clock, expiry persistence, Account
state, or response transport.

Account creation creates the account's first verifier and temporary
credential metadata in the same atomic state mutation that makes the account
available. It has no prior target sessions to revoke. The successful originating
internal result owns the generated temporary-password disclosure. The transport
consumes that value once under the approved wire contract; a lost
result requires a new explicit administrative action and does not trigger
automatic re-disclosure.

### Credential Issuance Assurance

Credential issuance has its own assurance profile. It accepts only an ordinary
current Administration session that has already passed Account authorization.
The authenticated Administrator re-presents their current password and, when a
TOTP factor is enrolled, the current six-digit TOTP code. A restricted
temporary-password session, an inactive actor, a temporary actor credential, a
wrong password, a missing or unexpected factor code, a disabled TOTP Module,
or stale actor credential state is denied without a ticket.

Successful password and factor verification creates a private proof bound to
the exact actor, current session digest, issuing Client Module, actor credential
revision, and factor observation. For an enrolled factor, the observation
contains the exact factor and verified TOTP time step. For an unenrolled actor,
it records the observed absence and TOTP Module state. Before publishing a
ticket, one Application Database transaction rechecks that exact session,
actor, revision, factor enrollment, and Module state and, when enrolled,
advances the replay watermark for the verified step. A stale or replayed result
publishes no ticket.

The returned ticket contains exactly 256 bits of operating-system randomness
encoded as 43 canonical unpadded Base64url characters. The process retains only
its domain-separated digest and proof in a bounded 64-entry memory store. The
ticket expires exactly five minutes after issuance according to the process's
monotonic clock; the exact expiry instant is invalid. It is not persisted, so a
Server restart invalidates every outstanding ticket.

Claim removes the entry before checking any later binding. A malformed,
unknown, expired, replayed, actor-mismatched, session-mismatched, or
Client-Module-mismatched ticket therefore authorizes no action, and a live
ticket presented with the wrong binding is still spent. Only one separately
authorized `Account(Create)` or `Account(PasswordReset)` action can consume the
proof. The consumed proof becomes a non-clonable admission whose exact action,
actor, session, Client Module, expected actor credential revision, and factor
observation feed the final writer recheck.

This proof is not `MfaStepUpProof`. It neither consumes nor extends that proof,
and it cannot authorize `MfaPolicy` or `GrantMutation`. Conversely, a current
five-minute `MfaStepUpProof` cannot substitute for the password reauthentication
and factor evidence required to disclose a temporary password.

Passwords, TOTP codes, ticket plaintext, and temporary passwords use clearing,
non-rendering owners at Server-controlled boundaries and are absent from Audit
and System Log content. Only the ticket's one response and a committed writer's
originating success response may disclose their respective values. A reported
denial and an indeterminate transport outcome carry no reason. Neither causes
automatic assurance, action, or disclosure retry; an explicit later reset
creates a new credential rather than recovering the earlier plaintext.

### Administration Step-Up Ticket

MFA requirement and enrollment-reset actions use the Administration action
gate's `MfaPolicy` family, not credential-issuance assurance. The public TOTP
step-up route accepts an ordinary Administrator session and exactly one
six-digit code. Server authentication opens that actor's current TOTP factor,
verifies the code, and asks the Application Database to atomically recheck the
exact session, active actor, factor ownership, Module enablement, and replay
watermark. Acceptance advances only the watermark and creates no session.

The resulting private `MfaStepUpProof` carries the exact actor, session digest,
factor, `MfaPolicy` family, monotonic issuance time, and exact five-minute
expiry. The browser cannot hold that capability directly, so the Server returns
a separate opaque ticket with 256 bits of randomness and retains only its
domain-separated digest and proof in a bounded 64-entry process-memory store.
A restart invalidates all entries. Ticket lookup does not spend a live proof:
the same exact-session ticket may authorize more than one matching policy
action during the fixed window, and every use re-enters the action gate for
actor, session, family, clock-rollback, and expiry checks.

The policy ticket is not persisted and never enters a cookie, URL, log, Audit
record, account projection, or credential-issuance workflow. It is distinct
from credential issuance's single-use password-plus-conditional-TOTP ticket.
The route accepts the closed `MfaPolicy` and `GrantMutation` families. Ticket
digests are domain-separated by family and each retained private proof carries
that same family. A cross-family, cross-session, expired, rolled-back,
malformed, or unknown ticket authorizes nothing. `GrantMutation` is exposed
only for public actions assigned that family, including empty Group deletion.

The final MFA policy writer receives the consumed authorized policy action and
rechecks the exact issuer session, Client Module, active actor, verified factor,
and TOTP Module enablement in its business transaction. Resetting the issuer's
factor or disabling its Module invalidates or revokes that session, so a stale
ticket cannot outlive the state that justified it.

### Password Reset Writer

1. **Issuance authorization:** An authenticated **[Administrator](../../glossary.md#identities-and-access)** with
   **[Server Administration Permission](../../glossary.md#identities-and-access)**
   passes Account authorization and the separate exact-session
   credential-issuance check: the current session reauthenticates with the
   Administrator's current password and, when enrolled, a TOTP code. Init is
   outside this flow.
2. **Generate and prepare:** The Server generates a temporary password using
   the same `PasswordVerifierFactory` as account creation, creates its Argon2
   verifier, and prepares the typed secret-bearing internal result before any
   single-use state mutation. The plaintext and response buffer use zeroizing,
   non-rendering owners; only the verifier is eligible for persistence.
3. **Audit sequencing:** The workflow requires a ready active recovery drain,
   preflights the exact current Audit generation, receives durable
   acknowledgement for a secret-free Attempt, and prepares every possible
   terminal obligation before mutation.
4. **Atomic mutation:** One compare-and-set transaction rechecks the exact
   issuer session, ordinary actor credential revision and state, current factor
   enrollment and Module enablement, and the verified TOTP replay step when
   enrolled. It then checks the expected
   account credential revision, writes or replaces the verifier, increments or
   replaces the revision, sets `must_change_password`, records the fixed
   24-hour absolute expiry, revokes the target's active sessions, and persists
   exactly the selected opaque Audit terminal. A stale concurrent request
   commits no credential and returns one stable, secret-free conflict result.
5. **One-response disclosure:** Only a committed successful mutation receives
   ownership of the plaintext temporary-password disclosure. Conflict, stale,
   denied, Audit-failure, and cancelled paths drop it. The value is never
   reconstructed or returned by a later lookup. An indeterminate or lost
   result is not automatically retried; a new reset creates a new credential
   and supersedes the old one.

### Temporary Credential Consumption

The Server implements temporary-credential consumption as an internal,
route-independent workflow. Account-create and password-reset transport is
owned by the Server API contract above. Password-change and forced-change
request and response envelopes, cookie mapping, client presentation, and public
identifier encoding remain separate from this consumption workflow.

1. **Temporary-password sign-in:** Before expiry, direct, TOTP, and required
   enrollment authentication paths may issue a session after all required MFA
   work completes. The Application Database derives that session's private
   `PasswordChangeRequired` posture from live Account state in the same
   transaction that validates and touches the session. The posture is not
   stored in the session row or exposed through the existing session response.
2. **Forced-change gate:** The private `ValidatedSession` proof retains the
   derived posture. Both ordinary authorization families accept only
   `Ordinary`; logout accepts either posture. Optional session-based MFA
   enrollment remains unavailable while password change is required.
3. **Password preparation:** The internal workflow consumes the exact
   restricted `ValidatedSession` by value and accepts one zeroizing non-empty
   replacement of at most 1,024 bytes. It rejects a replacement that verifies
   against the current temporary verifier, then creates only an approved-profile
   replacement verifier. It requests neither the temporary password nor a new
   TOTP code and does not advance an MFA replay watermark.
4. **Password change:** After the active Audit recovery sequence is ready, the
   workflow preflights the exact current Audit destination, obtains durable
   acknowledgement for a secret-free `authentication.password.changed`
   Attempt, and prepares success and denied terminals. One final transaction
   rechecks the exact session digest, actor, Client Module, lifetime, active
   Account, credential revision, `must_change_password`, unexpired temporary
   metadata, and current verifier. Success advances the revision, replaces the
   verifier, clears temporary state, revokes every old session, inserts one
   fresh ordinary session and CSRF pair, and stores only the success terminal.
   Any stale final state stores only the denied terminal. Audit persistence and
   fresh-session collisions roll back every business effect.
5. **Postcommit recovery:** A pending terminal after commit preserves both the
   committed password change and its fresh session result for the caller while
   leaving the exact terminal available to the bounded recovery drain.

The workflow adds no password-change continuation ticket. A future design that
adds one must specify it as a separate contract, must not confuse it with the
existing in-memory MFA continuation, and must not infer authority to persist it
from this internal workflow.

The session-validation response reports the live `password_change_required`
posture to the authenticated client. When it is true, the Web UI presents only
the replacement-password form and withholds ordinary User and Administration
workspaces. The browser sends the replacement once, clears its local input on
every outcome, and never retries an unreadable or timed-out request because the
transaction may already have committed. It may manually revalidate the session;
only a newly ordinary session unlocks the ordinary workspace.

**MFA-Ordering Invariant:** The forced-change gate MUST sit AFTER the MFA
step-up gate in an atomic transaction. This ensures that if MFA is required, the
user must complete MFA before reaching the password-change route, preventing
unauthorized password changes by actors with only the temporary password.

**State Semantics:** The `must_change_password` flag, temporary expiry, and
credential revision are stable Account properties and survive Server restarts
through the current Application Database schema. Creation creates no
prior sessions; reset revokes existing sessions. Any stale pre-reset password
verification or future password-change continuation cannot issue a session.
Changing the password clears the temporary metadata and flag and produces a
fresh session after required MFA. A missing or lost originating response has no
automatic retry or re-disclosure path; it requires a new reset. A self-reset
whose result is lost or expires can lock the Administrator out and, if that is
the last Administrator, make the deployment inaccessible through supported
interfaces. This is an accepted fail-closed risk; expiry recovery requires a
new Administrator reset and remains an operator responsibility.

### Account Disable And Re-enable

An authorized **[Administrator](../../glossary.md#identities-and-access)** may
disable or re-enable any local **[Human User](../../glossary.md#identities-and-access)**,
including themselves, through ordinary Account authorization. These status
operations do not issue a credential and therefore do not perform the
credential-issuance password reauthentication, TOTP verification, or an MFA
step-up. The final writer still rechecks the exact authorized session and active
actor before committing.

Disablement atomically marks the account inactive, advances its credential
revision, revokes every target session, and persists its selected Audit
terminal. Re-enablement marks the account active while preserving that advanced
revision and creates no session. The verifier, temporary-credential metadata,
MFA requirement and factors, and replay watermarks are preserved through both
operations.

Direct, TOTP, and enrollment session issuance already recheck active state and
the credential revision in the transaction that would issue a session. An
issuance prepared before disablement is therefore rejected while the account is
inactive and remains stale after re-enablement. Only authentication prepared
against the post-disable revision can issue a new session. This does not cancel
an unrelated request whose business operation already executed before the
disable transaction committed.

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

The store resolves the session, compares the presented CSRF digest, and
advances the last-seen instant as one atomic operation, and it advances that
instant only when the digests match. A request that fails the CSRF check is
therefore refused without extending the idle timeout, so a session token
presented without its bound CSRF token cannot keep a session alive. The refusal
is the same rejection an unknown session token produces.

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

The session and CSRF values and their rendered lines remain in controlled
application-owned clearing buffers. The renderer determines the two-line byte
length before one bounded allocation, and the listener composes its complete
response head from borrowed framing and those lines before delivery. The
listener owns that head through the head write, body write, and connection
shutdown; TLS, kernel, network-transport, and allocator copies remain outside
application control.

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
the shared Client Module contract crate (`weavelit-module-client`). That crate
also clears the collected login and second-factor request buffers when their
ownership ends, under the shared secret request-body contract the
[API Contract Design](../api/api-contract-design.md#secret-request-body-handling)
defines, including its defense-in-depth limit and its non-rejecting fallback
for a buffer it cannot own solely.

The `argon2` and `subtle` dependency records, and the authentication crate's use
of the already-approved `sha2`, `base64`, `getrandom`, and `zeroize`
dependencies, are recorded in the
[Server Architecture Design](../server-architecture-design.md#approved-production-dependencies).

## TOTP Multifactor Authentication

The compiled-in TOTP MFA Module's cryptographic profile, secret handling, and
provisioning URI construction are defined in the
[TOTP Module Design](../../mfa-modules/totp-module-design.md).

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

The two inputs this table reads from live state — whether the Module is enabled
and whether the account holds a factor for it — come from state loaded before
the password was verified, so the rows that issue a session directly do not act
on that reading. The decision and the session's insertion are one atomic step:
the Application Database contract's MFA store reads both inputs again inside
the transaction that writes the session, exactly as it does for a completed
second factor. A Module enabled between the table's reading and that write
therefore refuses the session instead of committing one, because enabling
revokes no session and the disablement that does revoke reaches only the
sessions that exist when it commits.

A login refused there loses no more than the direct session it would have
received. It is answered with the second-factor continuation the
enabled-and-enrolled row above already produces, because that is the row the
deployment is in once the enablement has committed, and the account holds the
factor that row asks for. No outcome, status, or reason exists for losing that
race, and nothing was written before the refusal. A factor enrolled in the same
window is covered by the same re-decision.

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

That five-minute lifetime is measured against a monotonic clock rather than
against the wall clock. Continuations are held only in the Server's memory and
are never persisted, so their deadlines need no durable representation, unlike
a session's, which is written in Unix milliseconds. Measuring them in wall-clock
time would let a system clock moved backwards extend a claimable continuation by
the rollback interval, keeping a password-verified or enrollment-confirming
bearer usable well past its documented window. A continuation therefore expires
after five minutes of elapsed time whatever the system clock does, and a
continuation refused because it has expired receives the single refusal every
other refused continuation receives, so no caller learns why its continuation
was refused.

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

That same operation also decides the Module's enablement and issues the
session. An enrollment is opened before it is confirmed, and the Module can be
disabled in between, so confirmation reads the enabled state, writes the factor
and its watermark, and writes the session inside one transaction, and refuses
the confirmation when the Module is not enabled. The refusal writes no factor
and issues no session, and is the same denial a login against a disabled Module
receives. Deciding enablement from state loaded before the write, or issuing the
session after the write commits, would only narrow that window: the confirmation
would persist a factor and issue a session behind a Module the deployment had
already stopped verifying, and because disabling revokes only the sessions that
exist when it commits, the newly issued session would survive the very operation
meant to end it. The continuation's own five-minute lifetime is unchanged;
enablement is an additional condition on the write, not a shorter window.

### Ordering Around The Confirmation Ticket

Opening an enrollment builds the secret and the complete provisioning URI, and
accepts that URI into the bounded type the response carries, before it issues
the one-time confirmation ticket. Nothing between issuing that ticket and
returning the response can refuse.

A caller therefore either receives the whole disclosure — the secret, a
conforming URI, and a ticket that confirms it — or receives a refusal that
consumed no claim and can simply open an enrollment again. Issuing the ticket
first would let any later refusal burn the one-time claim that ticket names,
which for an account required to use MFA is an unrecoverable lockout rather
than a retryable failure.

### Replay Watermark

An acceptance window that spans three time steps would otherwise let a code
observed in transit be presented again while it remains inside that window. The
Server therefore records, per enrolled factor, the highest time step it has
ever accepted. A presented code is accepted only when the step it matched is
strictly greater than that recorded watermark; a matched step at or below the
watermark is a replay and is refused even though the code is arithmetically
correct.

The check and the update are one operation, and so is everything the acceptance
decides. The Application Database contract exposes a backend-neutral MFA store
that reads the Module's enabled state, performs the watermark comparison and
write, and writes the session the acceptance issues, inside a single
transaction. No concurrent presentation of the same code can observe the
pre-update watermark and be accepted alongside the first, and a Module disabled
while the code was in flight cannot have a session issued behind it. The
decision belongs to the store rather than to a caller precisely because a
caller that read the watermark or the enabled state, decided, and then wrote
would reopen those windows. The enablement condition on a confirmed enrollment
belongs to the store for exactly the same reason.

A second factor presented after the Module was disabled is therefore refused by
the transaction that would have written the acceptance, and the refusal writes
no watermark and issues no session. Because the enabled state is read there, the
verification path performs no separate enablement check of its own beforehand: a
second check against separately loaded state would be a second decision, and
only the one inside the writing transaction can be authoritative. The refusal is
the same denial every other rejected code receives — the same stable error code,
the same response body shape, and the same absence of any cookie — so a Module
disabled mid-flight is indistinguishable from an incorrect or replayed code.

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

Changing enablement is a preview-then-act decision owned by the Server's
transport-independent TOTP administration workflow. It borrows an
`AuthorizedAdministrationAction` only for the exact
`ComponentEnablementChange(MfaModule, "totp", desired_state)` target when it
creates the preview, then consumes that action and the target-bound preview when
applying. The preview reports the number of distinct Human Users currently
holding a TOTP factor. The transaction recounts those Human Users and selects
either the prebuilt success Audit terminal or the payload-free conflict terminal.
A changed count commits only the conflict obligation, changes no enablement or
session state, and reports the current count. This satisfies the
[Security Model](../../security-model.md#multifactor-authentication-security-profile)'s
requirement to report the number of affected Human Users before disabling a
Module with dependent enrollments.

The TOTP Module's one mutable enablement authority is the generic component
entry whose component is `totp` and whose key is `mfa-module.enabled`. Init
explicitly seeds it disabled. Restore normalization emits exactly that entry,
and the SQLite compatibility migration maps and removes the former
`mfa.totp` / `enabled` entry. Enrollment, verification, direct-session issuance,
preview, and mutation therefore read or write the same canonical state.

Disabling the Module also revokes the live session of every account holding a
TOTP factor, in that same atomic operation, because those sessions were
established behind a factor this deployment is no longer willing to verify.
An enrollment already in flight cannot slip a session past that revocation,
because its confirmation is refused by the enablement condition described in
[Enrollment](#enrollment). Enabling the Module revokes no session, and needs
not: a login already in flight cannot commit a direct session past it, because
the enabled state and the account's enrollment are decided inside the
transaction that writes that session, as described in
[Second-Factor Admission](#second-factor-admission). Disablement never removes
an account's MFA requirement: a required account that holds no verifiable
factor is denied under the
[Second-Factor Admission](#second-factor-admission) table above rather than
admitted without one.

Before the transaction, the workflow requires the active Audit recovery
sequence to be ready, resolves and retains the exact current Audit configuration
generation, obtains the Administrator's typed Audit Reference, and waits for
durable Attempt acknowledgement. It prebuilds the applied and conflict terminal
obligations, then commits the selected obligation with recount, enablement, and
disablement session revocation in one transaction. After commit it invokes the
bounded active-then-late recovery drain and reports terminal delivery as
acknowledged or pending. A pending terminal never changes an applied result into
a rejection internally. The public
[TOTP Module Enablement Administration contract](../api/api-contract-design.md#totp-module-enablement-administration)
exposes it only through a specialized preview and apply pair. That pair uses an
ordinary Administration action so a disabled TOTP Module can be re-enabled; it
does not request or accept an `MfaPolicy`, `GrantMutation`, or
credential-issuance proof. The process retains the exact preview behind a
single-claim, digest-only, actor-, session-, Client Module-, and desired-state
bound credential. A public apply whose business change committed while terminal
delivery remains pending returns `service_unavailable` and does not claim
success.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Server Architecture Design](../server-architecture-design.md)
- [Server API Contract](../api/api-contract-design.md)
- [Server Restore Design](../lifecycle/restore/restore-design.md)
- [Authentication-Failure System Log Record](../observability/authentication-failure-record-design.md)
- [Temporary Password Disclosure Decision](temporary-password-disclosure-decision.md)
- [TOTP Module Design](../../mfa-modules/totp-module-design.md)
- [Glossary](../../glossary.md)
