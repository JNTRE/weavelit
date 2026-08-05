# Authentication Design

This document is the canonical destination for implementation-specific
authentication design for the **[Weavelit Server](../../glossary.md#applications-and-interfaces)**.
Binding application requirements remain in the
[Technical Specification](../../spec.md), and approved authentication and
session security profiles remain in the
[Security Model](../../security-model.md). This document owns how the Server
implements those requirements and profiles.

## Current Scope

This document owns the implementation design that satisfies the
[Security Model](../../security-model.md)'s Password And Session Security
Profile for **[Local Authentication](../../glossary.md#identities-and-access)**,
credential handling, sessions, and
**[Multifactor Authentication](../../glossary.md#identities-and-access)**, plus
optional **[External Authentication](../../glossary.md#identities-and-access)**.
Every concrete parameter, timing, and revocation trigger in that profile is
binding here by reference; this document must not restate, relax, or diverge
from those values.

Within that profile, this document will own: the maintained Argon2id library
selection; the versioned, allowlisted password verifier record layout and its
Password Hashing Competition (PHC)-style encoding; the parser's accepted
parameter and input-size bounds; the constant-time and dummy-verification
integration in the authentication control flow; the atomic rehash-on-success
lifecycle; the opaque session-token generation and stored verifier mechanics;
enforcement of idle, absolute, and fresh-authentication expiry and every
revocation trigger; session-token transport; authentication concurrency and
rate limiting; clock handling for expiry and lockout calculations; audit-event
production for authentication and session lifecycle actions; and error and
redaction mechanics for authentication failures.

This document must not weaken or independently set the approved password,
session, revocation, or reauthentication profile, and must not define
independent client-side password handling. It does not select crate versions
or Application Database schemas; those remain implementation detail resolved
during dependency selection and schema design work. No implementation-specific
decision has moved here yet.

## Related Documents

- [Security Model](../../security-model.md)
- [Technical Specification](../../spec.md)
- [Glossary](../../glossary.md)
