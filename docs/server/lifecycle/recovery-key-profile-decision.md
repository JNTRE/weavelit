# Recovery Key Profile Decision

## Status

Accepted.

## Context

**[Init](../../glossary.md#states-and-requests)** must deliver a backup
recovery key pair once, prove that the requesting client retained the private
key long enough to finalize Init, and never persist that private key. Backup
creation and **[Restore](../../glossary.md#states-and-requests)** must wrap and
unwrap a fresh per-backup data-encryption key against the same retained
recovery public key. The general backup container, framing, and versioned
format remain unresolved and are tracked in [Open Questions](../../open-questions.md#9-application-database-backup-format-and-staging),
so the recovery-key material and possession proof needed a profile that does
not prematurely fix that container. A browser-capable client must also be able
to perform key generation, the Init possession proof, and future backup
unwrapping without a heavy native cryptographic dependency.

Four options were evaluated for the recovery-key key material, format, and
Init possession proof:

- **Option A**: HPKE/X25519 v1 with versioned JWK artifacts, an HPKE-exporter
  Init proof, no Milestone 1 rotation, and AEAD integrity rather than
  Server-origin signing.
- **Option B**: `age` X25519 recipients and identities.
- **Option C**: RSA-3072 OAEP key pairs.
- **Option D**: An HPKE key pair combined with a separate Ed25519 signing key
  pair so the Init proof and future backups could also carry Server-origin
  signatures.

The user approved Option A for this decision.

## Decision

Weavelit approves HPKE (RFC 9180) base mode with DHKEM(X25519, HKDF-SHA-256)
(KEM `0x0020`), HKDF-SHA-256 (KDF `0x0001`), and ChaCha20-Poly1305 (AEAD
`0x0003`) as the recovery-key cryptographic profile. A version 1 public
recovery-key document is structured JSON with a public kind, a fixed profile
identifier, format version `1`, and an RFC 8037 X25519 public JWK; a version 1
private document carries the matching fixed fields, a private kind, and the
JWK `d` value, including or matching the public `x` value where required for
validation. Both `x` and `d` must be canonical unpadded Base64url and decode to
exactly 32 bytes. Key-document parsing bounds input, uses structured parsing,
and rejects duplicate, unknown, missing, unsupported, mismatched `d`/`x`,
wrong-length, noncanonical, low-order, and trailing-content input. Init
delivers the private document once as a compact UTF-8 copyable text artifact
that is never redisplayed, logged, staged, backed up, or persisted.

For the Init possession proof, the Server generates a 32-byte random nonce `N`
and the recovery pair `(sk_R, pk_R)`, binds the deployment identifier, profile
identifier, `pk_R`, and `N` into a checkpoint binding `B` using the canonical
transcript defined by the [Server Init Design](init/init-design.md), runs HPKE
`SetupBaseS` to `pk_R` with info `weavelit/init-proof/v1 || B` to produce `enc`,
and derives a 32-byte expected proof through HPKE Export with context
`weavelit/init-confirm/v1 || B`. The Init checkpoint persists only the profile
identifier, public key, nonce, and `enc`; the expected proof exists only in
zeroizing process memory. The client runs `SetupBaseR` and submits only the
canonical Base64url proof, which the Server compares in constant time before
its final commit. This proves decryption capability associated with the
checkpoint only, not durable storage, application identity, host control, or
authorization, and it fails across a different deployment or a regenerated
checkpoint.

For backup wrapping, every backup uses a fresh 32-byte data-encryption key that
HPKE seals to the retained recovery public key. Both the wrapping `info` and
the payload's authenticated associated data derive from the complete canonical
security header, binding the product, backup-format version, crypto profile,
payload profile, framing, and declared bounds. The recovery-key identifier
appears only inside authenticated ciphertext, never in cleartext header
metadata. The approved authenticity property is AEAD integrity and
confidentiality only; it detects tampering but does not prove Weavelit Server
origin, because this decision selects no origin-signing key.

Milestone 1 has no recovery-key rotation workflow. Restore preserves the
existing authenticated recovery public key, and a compatible existing or
future backup uses the same externally retained private key. This is a
deliberately limited current scope, not a permanent ban; rotation requires a
later custody, old-backup, and migration decision. The recovery key is never
an application identity, host-authority proof, authorization grant, or
at-rest key.

The [Security Model](../../security-model.md#recovery-key-security-profile) is
the current-policy authority for this profile; this record preserves why it
was selected and does not itself authorize a future change to that profile.

## Rejected Alternatives

- **`age` X25519** (Option B) was rejected for this profile because its
  recipient/identity envelope would prematurely select much of the still-open
  backup container and format, and it carries a heavier browser integration
  and cross-binding burden than a direct HPKE call.
- **RSA-3072 OAEP** (Option C) was rejected because its key and ciphertext
  artifacts are materially larger, its decryption cost is more attacker-
  influenced under adversarial input, and no current requirement needs its
  conventional PEM or Web Crypto interoperability over an X25519 JWK.
- **HPKE combined with a separate Ed25519 signing bundle** (Option D) was
  rejected as unnecessary additional key material, parser surface, and
  dependency surface solely to support the Init possession proof.
  Server-origin signing for the Init proof or for backups remains out of scope
  and unselected; a future decision may reconsider it independently.

## Consequences

Init, Restore, and the Application Database backup contract share one settled
key format, algorithm suite, and possession-proof mechanism, removing that
ambiguity from the still-open backup container and framing decision. Clients
can implement key generation, the Init proof, and future backup unwrapping with
one widely available HPKE/X25519 primitive rather than multiple cryptographic
stacks.

Decrypting a backup proves only possession of the matching private key, not
Server origin; any future requirement for origin authenticity needs a separate
signing-key decision and is not satisfied by this profile. Milestone 1 carries
no recovery-key rotation workflow; permanent loss of the private key or a
compromised key requires a new deployment and backup rather than in-place
recovery, and introducing rotation later requires a dedicated custody,
old-backup-compatibility, and migration decision. The general backup-format
envelope, framing, compatibility window, and normal-request staging policy
remain open in [Open Questions](../../open-questions.md#9-application-database-backup-format-and-staging).

## Related Documents

- [Security Model](../../security-model.md)
- [Server Init Design](init/init-design.md)
- [Server Restore Design](restore/restore-design.md)
- [Application Database Design](../database/application-database-design.md)
- [Open Questions](../../open-questions.md)
- [Technical Specification](../../spec.md)
