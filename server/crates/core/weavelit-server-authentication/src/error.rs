//! Payload-free authentication errors.
//!
//! An authentication failure must never distinguish a wrong password from an
//! unknown account, and an operational failure must never carry a stored
//! verifier, a password, a token, or a rejected encoded value. Every variant is
//! therefore a fixed unit value whose rendering is a fixed string.

use std::error::Error;
use std::fmt;

/// An operational failure raised while performing authentication work.
///
/// This type never reports an authentication decision. A denied credential is
/// [`crate::PasswordVerdict::Denied`], which is deliberately not an error, so
/// no caller can branch on an error to learn whether an account exists.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum AuthenticationError {
    /// Operating-system randomness was unavailable.
    ///
    /// Salt and token generation have no deterministic or lower-quality
    /// fallback, so the operation stops instead of producing a weaker value.
    RandomnessUnavailable,
    /// A password-hashing profile could not be applied.
    ///
    /// This covers a profile whose parameters the hashing library refuses and a
    /// proposed allowlist entry outside the approved verification ceiling. It
    /// never reports a rejected stored verifier; that is a denial.
    UnsupportedProfile,
    /// The hashing library failed to produce an encoded verifier.
    HashingFailed,
}

impl fmt::Display for AuthenticationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::RandomnessUnavailable => "operating-system randomness is unavailable",
            Self::UnsupportedProfile => "the password-hashing profile is not supported",
            Self::HashingFailed => "the password verifier could not be produced",
        };
        formatter.write_str(message)
    }
}

impl Error for AuthenticationError {}

#[cfg(test)]
mod tests {
    use super::AuthenticationError;

    #[test]
    fn every_error_renders_a_fixed_message_without_a_payload() {
        for error in [
            AuthenticationError::RandomnessUnavailable,
            AuthenticationError::UnsupportedProfile,
            AuthenticationError::HashingFailed,
        ] {
            let rendered = format!("{error} {error:?}");
            assert!(!rendered.contains('$'), "an error must not carry PHC text");
            assert!(
                rendered.is_ascii() && !rendered.contains('\n'),
                "an error message must stay a single fixed ASCII line"
            );
        }

        assert_eq!(
            AuthenticationError::RandomnessUnavailable.to_string(),
            "operating-system randomness is unavailable"
        );
        assert_eq!(
            AuthenticationError::UnsupportedProfile.to_string(),
            "the password-hashing profile is not supported"
        );
        assert_eq!(
            AuthenticationError::HashingFailed.to_string(),
            "the password verifier could not be produced"
        );
    }
}
