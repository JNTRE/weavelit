//! The one-time value that binds a verified password to its second factor.
//!
//! A login that verified a password but may not yet issue a session answers
//! with a continuation instead. The client presents that continuation on the
//! follow-up request, which is the only thing binding the two requests
//! together: it is independent cryptographically random bearer material and is
//! never a session identifier, an account identifier, or anything derived from
//! either. Nothing about the account can be recovered from it.
//!
//! A Server retains only [`ContinuationDigest`], never the continuation itself,
//! and compares a submitted value against that digest in constant time.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::AuthenticationError;
use crate::random::random_zeroizing_bytes;

/// Entropy one continuation carries, in bytes.
///
/// Thirty-two bytes is 256 bits, matching the session bearer values, because a
/// continuation authorizes the request that issues one.
pub const CONTINUATION_ENTROPY_BYTES: usize = 32;

/// Encoded length of a continuation in ASCII bytes.
///
/// Unpadded URL-safe Base64 of [`CONTINUATION_ENTROPY_BYTES`] is exactly this
/// many characters, all drawn from `[A-Za-z0-9_-]`.
pub const CONTINUATION_TEXT_BYTES: usize = 43;

const _: () = assert!(
    CONTINUATION_TEXT_BYTES == CONTINUATION_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the encoded continuation length must match unpadded Base64 of the approved entropy"
);

/// Domain separator so a continuation digest can never collide with another
/// digest this workspace computes over unrelated bytes.
const CONTINUATION_DIGEST_DOMAIN: &[u8] = b"weavelit.authentication.continuation.v1";

/// A minted one-time authentication continuation.
///
/// The value exists only long enough to be returned to the client that will
/// present it. It is cleared when dropped and renders through neither `Debug`
/// nor `Display`.
pub struct Continuation {
    text: Zeroizing<String>,
}

impl Continuation {
    /// Mints a continuation from operating-system randomness.
    pub fn generate() -> Result<Self, AuthenticationError> {
        Ok(Self::from_zeroizing_entropy(random_zeroizing_bytes::<
            CONTINUATION_ENTROPY_BYTES,
        >()?))
    }

    /// Encodes caller-supplied protected entropy as a continuation.
    ///
    /// Production code uses [`Self::generate`]; this entry point exists so a
    /// test can pin an exact encoded value.
    #[must_use]
    pub fn from_zeroizing_entropy(entropy: Zeroizing<[u8; CONTINUATION_ENTROPY_BYTES]>) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    /// Returns the encoded continuation for the one response that carries it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the digest a Server retains instead of the continuation.
    #[must_use]
    pub fn digest(&self) -> ContinuationDigest {
        ContinuationDigest::of(&self.text)
    }
}

impl fmt::Debug for Continuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Continuation(redacted)")
    }
}

impl fmt::Display for Continuation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Continuation(redacted)")
    }
}

/// The stored digest of one continuation.
///
/// This type deliberately implements neither `PartialEq` nor `Display`, so the
/// only available comparison is the constant-time [`ContinuationDigest::matches`]
/// and no code path can render it.
#[derive(Clone, Copy)]
pub struct ContinuationDigest([u8; 32]);

impl ContinuationDigest {
    /// Computes the domain-separated SHA-256 digest of a submitted value.
    #[must_use]
    pub fn of(continuation: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(CONTINUATION_DIGEST_DOMAIN);
        digest.update(continuation.as_bytes());
        Self(digest.finalize().into())
    }

    /// Compares two digests without a data-dependent branch or early return.
    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        let mut difference = 0_u8;
        for (stored, submitted) in self.0.iter().zip(other.0.iter()) {
            difference |= stored ^ submitted;
        }
        difference == 0
    }
}

impl fmt::Debug for ContinuationDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ContinuationDigest(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        CONTINUATION_ENTROPY_BYTES, CONTINUATION_TEXT_BYTES, Continuation, ContinuationDigest,
    };
    use zeroize::Zeroizing;

    fn seeded(seed: u8) -> Continuation {
        let mut entropy = [0_u8; CONTINUATION_ENTROPY_BYTES];
        entropy[0] = seed;
        Continuation::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    #[test]
    fn a_continuation_encodes_to_the_declared_length_and_character_set() {
        let continuation = seeded(1);

        assert_eq!(continuation.as_str().len(), CONTINUATION_TEXT_BYTES);
        assert!(
            continuation
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
    }

    #[test]
    fn generated_continuations_do_not_repeat() {
        let minted: BTreeSet<String> = (0..8)
            .map(|_| {
                Continuation::generate()
                    .expect("host randomness must be available")
                    .as_str()
                    .to_owned()
            })
            .collect();

        assert_eq!(minted.len(), 8);
    }

    #[test]
    fn a_digest_matches_only_its_own_continuation() {
        let issued = seeded(2);
        let other = seeded(3);

        assert!(
            issued
                .digest()
                .matches(&ContinuationDigest::of(issued.as_str()))
        );
        assert!(!issued.digest().matches(&other.digest()));
    }

    #[test]
    fn the_digest_is_domain_separated_from_a_plain_digest_of_the_same_text() {
        use sha2::{Digest as _, Sha256};

        let continuation = seeded(4);
        let undomained: [u8; 32] = Sha256::digest(continuation.as_str().as_bytes()).into();

        assert!(
            !continuation
                .digest()
                .matches(&ContinuationDigest(undomained))
        );
    }

    #[test]
    fn neither_the_continuation_nor_its_digest_renders_its_value() {
        let continuation = seeded(5);

        assert_eq!(format!("{continuation:?}"), "Continuation(redacted)");
        assert_eq!(format!("{continuation}"), "Continuation(redacted)");
        assert_eq!(
            format!("{:?}", continuation.digest()),
            "ContinuationDigest(redacted)"
        );
    }
}
