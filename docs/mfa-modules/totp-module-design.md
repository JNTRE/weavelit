# TOTP Module Design

This document is the canonical destination for the method-specific design of
the compiled-in
**[Time-Based One-Time Password (TOTP)](../glossary.md#identities-and-access)**
**[MFA Module](../glossary.md#applications-and-interfaces)**: its cryptographic
profile, secret handling, and provisioning URI construction. It does not define
MFA policy, second-factor admission, session issuance, enrollment
orchestration, Module enablement, recovery, or audit behavior; those remain
**[Weavelit Server](../glossary.md#applications-and-interfaces)**
responsibilities defined in the
[Authentication Design](../server/authentication/authentication-design.md#totp-multifactor-authentication).

## Profile And Secret Handling

The compiled-in **[Time-Based One-Time Password (TOTP)](../glossary.md#identities-and-access)**
**[MFA Module](../glossary.md#applications-and-interfaces)** uses the `totp-rs`
library and the RFC 6238 profile: HMAC-SHA-1, 6 digits, a 30-second period, and
`T0=0`. A secret is a random 160-bit value stored as unpadded RFC 4648 Base32
and is provisioned through an `otpauth://` URI disclosed exactly once. The
Server supplies the twenty secret bytes from the operating-system random source
directly into zeroizing storage; the Module never generates them. Fresh
enrollment retains a separate zeroizing copy only for the pending confirmation,
and decrypted factor data is copied into zeroizing storage before the Module
adopts it. The Module holds the secret and the provisioning URI in
zeroizing types that redact in `Debug`, so neither can reach a log, an error,
or a response body except through an explicit disclosure. Verification accepts
the current time step and one step on either side. The Module derives and
compares codes only; it reads no clock, takes the verification time as a
parameter, and owns no policy, session, recovery, or audit behavior. This
implements the
[Security Model](../security-model.md#multifactor-authentication-security-profile)'s
enrollment and disclosure requirements.

## Provisioning URI Construction

The URI's issuer, secret, and profile parameters are exact: the issuer is the
deployment's fixed provisioning issuer, and the `secret`, `algorithm`,
`digits`, and `period` parameters carry the enrolled values unchanged. The
account portion of the URI's label is cosmetic. An authenticator displays it,
and nothing about verification, enrollment, or the account's canonical username
depends on it.

The Server therefore fits the account label to the byte bound its response
envelope enforces on a disclosed URI rather than refusing a name that will not
fit. A label that already fits is percent-encoded and carried unchanged, so an
ordinary account name produces exactly the URI it always has. A longer one is
encoded one Unicode scalar at a time and cut only on a scalar boundary, so the
result never ends inside a multi-byte character or a partial percent escape,
and a trailing `~` marks that it was shortened. A colon in an account name is
shown as an unreserved substitute, because a colon is what separates the issuer
from the account in the label itself. A name that leaves nothing displayable
falls back to a short fixed label. Construction therefore succeeds for every
accepted account name.

This is deliberate. The account-name bound and the URI bound are set
independently, so an accepted username can be longer than a conforming URI can
carry. Refusing that enrollment would leave an account that is required to hold
a second factor permanently unable to sign in, which is a far worse outcome
than an authenticator showing a shortened display label. No account name
produces an enrollment-specific refusal or a distinguishing error code.

## Related Documents

- [Authentication Design](../server/authentication/authentication-design.md)
- [Security Model](../security-model.md)
- [Glossary](../glossary.md)
