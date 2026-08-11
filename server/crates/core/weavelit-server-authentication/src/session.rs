//! Session and CSRF bearer values and the digests the Server persists.
//!
//! A session token and its paired CSRF token are opaque random values. The
//! Application Database stores only their SHA-256 digests, so a database read
//! never yields a usable bearer value. Neither token renders through `Debug` or
//! `Display`, and a digest is comparable only in constant time.
//!
//! This module produces and hashes these values. It does not persist them and
//! does not emit cookies.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest as _, Sha256};
use subtle::ConstantTimeEq as _;
use zeroize::Zeroizing;

use crate::error::AuthenticationError;
use crate::random::random_bytes;

/// Entropy one session or CSRF token carries, in bytes.
pub const SESSION_TOKEN_ENTROPY_BYTES: usize = 32;

/// Encoded length of a session or CSRF token in ASCII characters.
///
/// Unpadded URL-safe Base64 of [`SESSION_TOKEN_ENTROPY_BYTES`] is exactly this
/// many characters, all drawn from `[A-Za-z0-9_-]`.
pub const SESSION_TOKEN_TEXT_BYTES: usize = 43;

const _: () = assert!(
    SESSION_TOKEN_TEXT_BYTES == SESSION_TOKEN_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the encoded token length must match unpadded Base64 of the approved entropy"
);

/// Domain separator for a session-token digest.
const SESSION_DIGEST_DOMAIN: &[u8] = b"weavelit.session.token.v1";

/// Domain separator for a CSRF-token digest.
const CSRF_DIGEST_DOMAIN: &[u8] = b"weavelit.session.csrf.v1";

macro_rules! bearer_token {
    (
        $token:ident,
        $digest:ident,
        $domain:ident,
        $token_doc:literal,
        $digest_doc:literal
    ) => {
        #[doc = $token_doc]
        ///
        /// The encoded value is cleared when dropped and renders through
        /// neither `Debug` nor `Display`.
        pub struct $token {
            text: Zeroizing<String>,
        }

        impl $token {
            /// Generates a token from operating-system randomness.
            pub fn generate() -> Result<Self, AuthenticationError> {
                Ok(Self::from_entropy(random_bytes::<
                    SESSION_TOKEN_ENTROPY_BYTES,
                >()?))
            }

            /// Encodes caller-supplied entropy as a token.
            ///
            /// Production code uses [`Self::generate`]; this entry point exists
            /// so a test can pin an exact encoded value.
            #[must_use]
            pub fn from_entropy(entropy: [u8; SESSION_TOKEN_ENTROPY_BYTES]) -> Self {
                Self {
                    text: Zeroizing::new(URL_SAFE_NO_PAD.encode(entropy)),
                }
            }

            /// Returns the encoded token for the one response that carries it.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.text
            }

            /// Returns the digest the Server stores instead of the token.
            #[must_use]
            pub fn digest(&self) -> $digest {
                $digest::of(&self.text)
            }
        }

        impl fmt::Debug for $token {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($token), "(redacted)"))
            }
        }

        impl fmt::Display for $token {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($token), "(redacted)"))
            }
        }

        #[doc = $digest_doc]
        ///
        /// This type implements neither `PartialEq` nor `Display`, so the only
        /// available comparison is the constant-time [`Self::matches`] and no
        /// code path can render it.
        #[derive(Clone, Copy)]
        pub struct $digest([u8; 32]);

        impl $digest {
            /// Computes the domain-separated SHA-256 digest of a token.
            #[must_use]
            pub fn of(token: &str) -> Self {
                let mut digest = Sha256::new();
                digest.update($domain);
                digest.update(token.as_bytes());
                Self(digest.finalize().into())
            }

            /// Returns the stored digest bytes for persistence.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 32] {
                &self.0
            }

            /// Reconstructs a digest read back from storage.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 32]) -> Self {
                Self(bytes)
            }

            /// Compares two digests without a data-dependent branch.
            #[must_use]
            pub fn matches(&self, other: &Self) -> bool {
                self.0.ct_eq(&other.0).into()
            }
        }

        impl fmt::Debug for $digest {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(concat!(stringify!($digest), "(redacted)"))
            }
        }
    };
}

bearer_token!(
    SessionToken,
    SessionTokenDigest,
    SESSION_DIGEST_DOMAIN,
    "An opaque session bearer token.",
    "The stored digest of one session token."
);

bearer_token!(
    CsrfToken,
    CsrfTokenDigest,
    CSRF_DIGEST_DOMAIN,
    "An opaque per-session cross-site request forgery token.",
    "The stored digest of one CSRF token."
);

/// One session's freshly minted bearer values.
///
/// The two tokens are independent random values; neither is derived from the
/// other, so disclosing the CSRF token to the browser cannot reveal the session
/// token.
pub struct SessionSecrets {
    session: SessionToken,
    csrf: CsrfToken,
}

impl SessionSecrets {
    /// Generates an independent session token and CSRF token.
    pub fn generate() -> Result<Self, AuthenticationError> {
        Ok(Self {
            session: SessionToken::generate()?,
            csrf: CsrfToken::generate()?,
        })
    }

    /// Returns the session token.
    #[must_use]
    pub const fn session(&self) -> &SessionToken {
        &self.session
    }

    /// Returns the per-session CSRF token.
    #[must_use]
    pub const fn csrf(&self) -> &CsrfToken {
        &self.csrf
    }

    /// Returns the two digests the Server persists for this session.
    #[must_use]
    pub fn digests(&self) -> (SessionTokenDigest, CsrfTokenDigest) {
        (self.session.digest(), self.csrf.digest())
    }
}

impl fmt::Debug for SessionSecrets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SessionSecrets(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        CsrfToken, CsrfTokenDigest, SESSION_TOKEN_ENTROPY_BYTES, SESSION_TOKEN_TEXT_BYTES,
        SessionSecrets, SessionToken, SessionTokenDigest,
    };

    fn seeded(seed: u8) -> [u8; SESSION_TOKEN_ENTROPY_BYTES] {
        let mut entropy = [0_u8; SESSION_TOKEN_ENTROPY_BYTES];
        for (index, byte) in entropy.iter_mut().enumerate() {
            *byte = seed
                .wrapping_mul(31)
                .wrapping_add(u8::try_from(index).unwrap_or(u8::MAX));
        }
        entropy
    }

    #[test]
    fn a_token_encodes_its_full_entropy_as_unpadded_base64url() {
        let session = SessionToken::from_entropy([0_u8; SESSION_TOKEN_ENTROPY_BYTES]);
        assert_eq!(
            session.as_str(),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
        );

        let session = SessionToken::from_entropy(seeded(7));
        assert_eq!(session.as_str().len(), SESSION_TOKEN_TEXT_BYTES);
        assert!(
            session
                .as_str()
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'),
            "a token must never require escaping in a cookie or JSON body"
        );

        let csrf = CsrfToken::from_entropy(seeded(7));
        assert_eq!(csrf.as_str(), session.as_str());
    }

    #[test]
    fn generated_tokens_are_distinct_and_carry_full_entropy() {
        let tokens: BTreeSet<String> = (0..16)
            .map(|_| {
                SessionToken::generate()
                    .expect("host randomness must be available")
                    .as_str()
                    .to_owned()
            })
            .collect();
        assert_eq!(tokens.len(), 16);
        for token in &tokens {
            assert_eq!(token.len(), SESSION_TOKEN_TEXT_BYTES);
        }
    }

    #[test]
    fn a_digest_matches_only_its_own_token() {
        let token = SessionToken::from_entropy(seeded(3));
        let digest = token.digest();
        assert!(digest.matches(&SessionTokenDigest::of(token.as_str())));
        assert!(!digest.matches(&SessionTokenDigest::of("")));
        assert!(!digest.matches(&SessionToken::from_entropy(seeded(4)).digest()));

        let mut altered = token.as_str().to_owned();
        altered.replace_range(0..1, if altered.starts_with('A') { "B" } else { "A" });
        assert!(!digest.matches(&SessionTokenDigest::of(&altered)));

        let restored = SessionTokenDigest::from_bytes(*digest.as_bytes());
        assert!(digest.matches(&restored));
    }

    #[test]
    fn the_session_and_csrf_digests_are_domain_separated() {
        let entropy = seeded(11);
        let session = SessionToken::from_entropy(entropy);
        let csrf = CsrfToken::from_entropy(entropy);
        assert_eq!(session.as_str(), csrf.as_str());
        assert_ne!(
            session.digest().as_bytes(),
            csrf.digest().as_bytes(),
            "identical bytes must not produce the same digest in both domains"
        );
        assert_ne!(
            session.digest().as_bytes(),
            CsrfTokenDigest::of(session.as_str()).as_bytes()
        );
    }

    #[test]
    fn a_digest_is_the_domain_separated_sha256_of_the_token() {
        use sha2::{Digest as _, Sha256};

        let token = SessionToken::from_entropy(seeded(5));
        let mut expected = Sha256::new();
        expected.update(b"weavelit.session.token.v1");
        expected.update(token.as_str().as_bytes());
        let expected: [u8; 32] = expected.finalize().into();
        assert_eq!(token.digest().as_bytes(), &expected);

        let csrf = CsrfToken::from_entropy(seeded(5));
        let mut expected = Sha256::new();
        expected.update(b"weavelit.session.csrf.v1");
        expected.update(csrf.as_str().as_bytes());
        let expected: [u8; 32] = expected.finalize().into();
        assert_eq!(csrf.digest().as_bytes(), &expected);
    }

    #[test]
    fn neither_a_token_nor_a_digest_renders_its_value() {
        let secrets = SessionSecrets::generate().expect("host randomness must be available");
        let session = secrets.session();
        let csrf = secrets.csrf();
        let (session_digest, csrf_digest) = secrets.digests();

        let rendered = format!(
            "{session:?} {session} {csrf:?} {csrf} {session_digest:?} {csrf_digest:?} {secrets:?}"
        );
        assert!(!rendered.contains(session.as_str()));
        assert!(!rendered.contains(csrf.as_str()));
        assert_eq!(
            rendered,
            "SessionToken(redacted) SessionToken(redacted) CsrfToken(redacted) \
             CsrfToken(redacted) SessionTokenDigest(redacted) CsrfTokenDigest(redacted) \
             SessionSecrets(redacted)"
        );
    }

    #[test]
    fn a_session_pairs_two_independent_tokens() {
        let secrets = SessionSecrets::generate().expect("host randomness must be available");
        assert_ne!(secrets.session().as_str(), secrets.csrf().as_str());

        let (session_digest, csrf_digest) = secrets.digests();
        assert!(session_digest.matches(&secrets.session().digest()));
        assert!(csrf_digest.matches(&secrets.csrf().digest()));
    }
}
