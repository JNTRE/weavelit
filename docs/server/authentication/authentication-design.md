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
salt, and a 32-byte output, and are stored in PHC string format. After a
successful authentication, the Server rehashes and replaces the stored
verifier whenever its encoded parameters differ from the currently configured
parameters. This satisfies the
[Security Model](../../security-model.md#password-and-session-security-profile)'s
adaptive password-hashing requirement.

## Session Representation

A session token is an opaque 32-byte random value encoded as unpadded
Base64url. The Application Database stores only the token's SHA-256 hash and
its lifecycle metadata; the plaintext token is never persisted. The Server
issues the session in a `__Host-weavelit_session` cookie with `Secure`,
`HttpOnly`, `SameSite=Strict`, `Path=/`, and no `Domain` attribute.

The session uses a balanced lifetime policy: a 30-minute idle timeout, a
12-hour absolute maximum, and a browser-session cookie carrying no `Max-Age`
or `Expires` attribute.

## Cross-Site Request Forgery Protection

Each session has a separate random per-session CSRF token; the Server stores
only its SHA-256 hash alongside the session. The token is exposed to the
browser through a secure, same-site readable cookie and is required in the
`X-Weavelit-CSRF` header on every mutating request, alongside same-origin
validation. The Server rotates the CSRF token on login and on MFA or privilege
elevation.

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
- [Glossary](../../glossary.md)
