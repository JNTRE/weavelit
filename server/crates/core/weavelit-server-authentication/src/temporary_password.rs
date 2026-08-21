//! Preparation of one-response temporary password credentials.
//!
//! Account creation and password reset can prepare a plaintext disclosure and
//! its approved verifier before beginning their future state mutation. Only
//! the verifier is persistable; the disclosure is a clearing, consuming value
//! for the one successful response that is allowed to carry it.

use std::fmt;
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

use crate::error::AuthenticationError;
use crate::random::random_zeroizing_bytes;
use crate::verifier::{NewPasswordVerifier, PasswordVerifierFactory};

/// Entropy in one generated temporary password, in bytes.
pub const TEMPORARY_PASSWORD_ENTROPY_BYTES: usize = 18;

/// Encoded length of one temporary password in ASCII bytes.
pub const TEMPORARY_PASSWORD_TEXT_BYTES: usize = 24;

/// Absolute lifetime future account workflows assign at issuance.
pub const TEMPORARY_PASSWORD_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60);

const _: () = assert!(
    TEMPORARY_PASSWORD_TEXT_BYTES == TEMPORARY_PASSWORD_ENTROPY_BYTES / 3 * 4,
    "the temporary password length must match unpadded Base64 of the approved entropy"
);

/// A prepared temporary password and the verifier eligible for persistence.
///
/// Construction completes generation and hashing before a future caller starts
/// its state mutation. The bundle is not clonable, so its disclosure has one
/// owner throughout that workflow.
///
/// ```compile_fail
/// use weavelit_server_authentication::PreparedTemporaryPassword;
///
/// fn duplicate(prepared: PreparedTemporaryPassword) {
///     let _duplicate = prepared.clone();
/// }
/// ```
pub struct PreparedTemporaryPassword {
    verifier: NewPasswordVerifier,
    disclosure: TemporaryPasswordDisclosure,
}

impl PreparedTemporaryPassword {
    /// Generates and hashes one temporary password at the approved profile.
    ///
    /// # Errors
    ///
    /// Returns a payload-free [`AuthenticationError`] when operating-system
    /// randomness or verifier creation fails. No partial bundle is returned.
    pub fn generate(factory: &PasswordVerifierFactory) -> Result<Self, AuthenticationError> {
        let disclosure = TemporaryPasswordDisclosure::generate()?;
        let verifier = factory.create(disclosure.secret_bytes())?;
        Ok(Self {
            verifier,
            disclosure,
        })
    }

    /// Returns the verifier a future transaction may persist.
    #[must_use]
    pub fn verifier(&self) -> &NewPasswordVerifier {
        &self.verifier
    }

    /// Separates the persistable verifier from the one-response disclosure.
    #[must_use]
    pub fn into_parts(self) -> (NewPasswordVerifier, TemporaryPasswordDisclosure) {
        (self.verifier, self.disclosure)
    }
}

impl fmt::Debug for PreparedTemporaryPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedTemporaryPassword(redacted)")
    }
}

/// The temporary password allowed into one successful originating response.
///
/// This value is not clonable and has no borrowed plaintext accessor. Its only
/// disclosure operation consumes it and transfers the clearing string to the
/// future response owner.
pub struct TemporaryPasswordDisclosure {
    secret: Zeroizing<String>,
}

impl TemporaryPasswordDisclosure {
    fn generate() -> Result<Self, AuthenticationError> {
        Self::generate_with(random_zeroizing_bytes::<TEMPORARY_PASSWORD_ENTROPY_BYTES>)
    }

    fn generate_with<F>(entropy_source: F) -> Result<Self, AuthenticationError>
    where
        F: FnOnce()
            -> Result<Zeroizing<[u8; TEMPORARY_PASSWORD_ENTROPY_BYTES]>, AuthenticationError>,
    {
        let entropy = entropy_source()?;
        if entropy.iter().all(|byte| *byte == 0) {
            return Err(AuthenticationError::RandomnessUnavailable);
        }

        Ok(Self {
            secret: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        })
    }

    fn secret_bytes(&self) -> &[u8] {
        self.secret.as_bytes()
    }

    /// Transfers the temporary password to its one response owner.
    ///
    /// The returned string clears its allocation when dropped. Because this
    /// method consumes the disclosure, a caller cannot invoke it twice.
    ///
    /// ```compile_fail
    /// use weavelit_server_authentication::TemporaryPasswordDisclosure;
    ///
    /// fn disclose_twice(disclosure: TemporaryPasswordDisclosure) {
    ///     let _first = disclosure.into_secret();
    ///     let _second = disclosure.into_secret();
    /// }
    /// ```
    #[must_use]
    pub fn into_secret(self) -> Zeroizing<String> {
        self.secret
    }
}

impl fmt::Debug for TemporaryPasswordDisclosure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TemporaryPasswordDisclosure(redacted)")
    }
}

impl fmt::Display for TemporaryPasswordDisclosure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TemporaryPasswordDisclosure(redacted)")
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use zeroize::Zeroizing;

    use super::{
        PreparedTemporaryPassword, TEMPORARY_PASSWORD_ENTROPY_BYTES, TEMPORARY_PASSWORD_LIFETIME,
        TEMPORARY_PASSWORD_TEXT_BYTES, TemporaryPasswordDisclosure,
    };
    use crate::engine::{Argon2Engine as _, RustCryptoArgon2};
    use crate::error::AuthenticationError;
    use crate::profile::CURRENT_ARGON2_PROFILE;
    use crate::verifier::PasswordVerifierFactory;

    fn seeded(seed: u8) -> TemporaryPasswordDisclosure {
        TemporaryPasswordDisclosure::generate_with(|| {
            let mut entropy = [0_u8; TEMPORARY_PASSWORD_ENTROPY_BYTES];
            entropy[0] = seed;
            Ok(Zeroizing::new(entropy))
        })
        .expect("nonzero entropy must produce a disclosure")
    }

    #[test]
    fn temporary_passwords_have_the_declared_entropy_length_and_alphabet() {
        let disclosure = seeded(1);
        let secret = disclosure.into_secret();

        assert_eq!(TEMPORARY_PASSWORD_ENTROPY_BYTES * 8, 144);
        assert_eq!(secret.len(), TEMPORARY_PASSWORD_TEXT_BYTES);
        assert!(
            secret
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        );
        assert!(!secret.contains('='));
    }

    #[test]
    fn distinct_entropy_produces_distinct_nonzero_passwords() {
        let generated: Vec<Zeroizing<String>> =
            (1..=8).map(|seed| seeded(seed).into_secret()).collect();

        assert_eq!(generated.len(), 8);
        assert!(generated.iter().all(|secret| !secret.is_empty()));
        for (index, secret) in generated.iter().enumerate() {
            assert!(generated[index + 1..].iter().all(|other| secret != other));
        }
    }

    #[test]
    fn operating_system_generation_never_returns_the_all_zero_encoding() {
        let generated = TemporaryPasswordDisclosure::generate()
            .expect("host randomness must be available")
            .into_secret();

        assert_ne!(generated.as_str(), "AAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn entropy_failure_and_all_zero_entropy_fail_without_a_secret() {
        let unavailable = TemporaryPasswordDisclosure::generate_with(|| {
            Err(AuthenticationError::RandomnessUnavailable)
        });
        let all_zero = TemporaryPasswordDisclosure::generate_with(|| {
            Ok(Zeroizing::new([0_u8; TEMPORARY_PASSWORD_ENTROPY_BYTES]))
        });

        assert!(matches!(
            unavailable,
            Err(AuthenticationError::RandomnessUnavailable)
        ));
        assert!(matches!(
            all_zero,
            Err(AuthenticationError::RandomnessUnavailable)
        ));
    }

    #[test]
    fn a_prepared_verifier_accepts_only_its_generated_password() {
        let prepared = PreparedTemporaryPassword::generate(&PasswordVerifierFactory::approved())
            .expect("temporary password preparation must succeed");
        let (verifier, disclosure) = prepared.into_parts();
        let secret = disclosure.into_secret();
        let engine = RustCryptoArgon2::default();

        assert!(engine.verify(
            secret.as_bytes(),
            &CURRENT_ARGON2_PROFILE,
            verifier.as_str()
        ));
        assert!(!engine.verify(
            b"different temporary password",
            &CURRENT_ARGON2_PROFILE,
            verifier.as_str()
        ));
        assert!(!verifier.as_str().contains(secret.as_str()));
    }

    #[test]
    fn temporary_password_types_render_only_fixed_redaction() {
        let disclosure = seeded(2);
        let secret = disclosure.secret.to_string();

        assert_eq!(
            format!("{disclosure:?}"),
            "TemporaryPasswordDisclosure(redacted)"
        );
        assert_eq!(
            format!("{disclosure}"),
            "TemporaryPasswordDisclosure(redacted)"
        );
        assert!(!format!("{disclosure:?} {disclosure}").contains(&secret));

        let prepared = PreparedTemporaryPassword {
            verifier: PasswordVerifierFactory::approved()
                .create(b"not the temporary password")
                .expect("verifier creation must succeed"),
            disclosure,
        };
        assert_eq!(
            format!("{prepared:?}"),
            "PreparedTemporaryPassword(redacted)"
        );
        assert!(!format!("{prepared:?}").contains(&secret));
    }

    #[test]
    fn authentication_errors_never_carry_temporary_password_text() {
        let secret = seeded(3).into_secret();

        for error in [
            AuthenticationError::RandomnessUnavailable,
            AuthenticationError::UnsupportedProfile,
            AuthenticationError::HashingFailed,
        ] {
            assert!(!format!("{error} {error:?}").contains(secret.as_str()));
        }
    }

    #[test]
    fn expiry_policy_is_exactly_twenty_four_hours() {
        assert_eq!(TEMPORARY_PASSWORD_LIFETIME, Duration::from_secs(86_400));
    }
}
