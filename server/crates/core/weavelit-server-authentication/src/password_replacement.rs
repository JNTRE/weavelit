//! Preparation of a normal password that replaces a temporary credential.

use std::fmt;

use zeroize::Zeroizing;

use crate::{
    Argon2Engine, NewPasswordVerifier, PasswordAuthenticator, PasswordVerdict,
    PasswordVerifierFactory, StoredCredential,
};

/// Largest accepted replacement password, in bytes.
pub const MAX_PASSWORD_REPLACEMENT_BYTES: usize = 1024;

/// Clearing replacement-password input owned for one preparation attempt.
pub struct PasswordReplacementInput {
    password: Zeroizing<Vec<u8>>,
}

impl PasswordReplacementInput {
    /// Adopts a bounded, non-empty replacement password.
    pub fn new(password: Zeroizing<Vec<u8>>) -> Result<Self, PasswordReplacementError> {
        if password.is_empty() || password.len() > MAX_PASSWORD_REPLACEMENT_BYTES {
            return Err(PasswordReplacementError::InvalidInput);
        }
        Ok(Self { password })
    }

    fn as_bytes(&self) -> &[u8] {
        self.password.as_slice()
    }
}

impl fmt::Debug for PasswordReplacementInput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PasswordReplacementInput(REDACTED)")
    }
}

/// Approved verifier prepared after the replacement differs from the temporary password.
pub struct PreparedPasswordReplacement {
    verifier: NewPasswordVerifier,
}

impl PreparedPasswordReplacement {
    /// Verifies the replacement does not match the current verifier, then hashes it.
    pub fn prepare<E: Argon2Engine>(
        authenticator: &PasswordAuthenticator<E>,
        current_verifier: &str,
        replacement: PasswordReplacementInput,
    ) -> Result<Self, PasswordReplacementError> {
        match authenticator
            .authenticate(
                StoredCredential::Verifier(current_verifier),
                replacement.as_bytes(),
            )
            .map_err(|_| PasswordReplacementError::Unavailable)?
        {
            PasswordVerdict::Verified { .. } => return Err(PasswordReplacementError::SamePassword),
            PasswordVerdict::Denied => {}
        }
        let verifier = PasswordVerifierFactory::approved()
            .create(replacement.as_bytes())
            .map_err(|_| PasswordReplacementError::Unavailable)?;
        Ok(Self { verifier })
    }

    /// Consumes the preparation into the only persistable value it retains.
    #[must_use]
    pub fn into_verifier(self) -> NewPasswordVerifier {
        self.verifier
    }
}

impl fmt::Debug for PreparedPasswordReplacement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedPasswordReplacement(REDACTED)")
    }
}

/// Payload-free replacement validation or preparation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordReplacementError {
    /// The replacement was empty or exceeded the input bound.
    InvalidInput,
    /// The replacement matched the current temporary credential.
    SamePassword,
    /// Password verification or verifier creation could not complete safely.
    Unavailable,
}

impl fmt::Display for PasswordReplacementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidInput => "password replacement input is invalid",
            Self::SamePassword => "password replacement is denied",
            Self::Unavailable => "password replacement is unavailable",
        })
    }
}

impl std::error::Error for PasswordReplacementError {}

#[cfg(test)]
mod tests {
    use zeroize::Zeroizing;

    use super::*;
    use crate::{PasswordPolicy, RustCryptoArgon2};

    fn authenticator() -> PasswordAuthenticator<RustCryptoArgon2> {
        PasswordAuthenticator::new(RustCryptoArgon2::default(), PasswordPolicy::approved()).unwrap()
    }

    fn verifier(password: &[u8]) -> NewPasswordVerifier {
        PasswordVerifierFactory::approved()
            .create(password)
            .unwrap()
    }

    #[test]
    fn replacement_input_accepts_only_the_non_empty_bounded_range() {
        assert!(PasswordReplacementInput::new(Zeroizing::new(vec![1])).is_ok());
        assert!(
            PasswordReplacementInput::new(Zeroizing::new(vec![1; MAX_PASSWORD_REPLACEMENT_BYTES]))
                .is_ok()
        );
        assert_eq!(
            PasswordReplacementInput::new(Zeroizing::new(Vec::new())).unwrap_err(),
            PasswordReplacementError::InvalidInput
        );
        assert_eq!(
            PasswordReplacementInput::new(Zeroizing::new(vec![
                1;
                MAX_PASSWORD_REPLACEMENT_BYTES + 1
            ]))
            .unwrap_err(),
            PasswordReplacementError::InvalidInput
        );
    }

    #[test]
    fn the_current_temporary_password_is_rejected() {
        let temporary = b"temporary credential";
        let current = verifier(temporary);

        let error = PreparedPasswordReplacement::prepare(
            &authenticator(),
            current.as_str(),
            PasswordReplacementInput::new(Zeroizing::new(temporary.to_vec())).unwrap(),
        )
        .unwrap_err();

        assert_eq!(error, PasswordReplacementError::SamePassword);
    }

    #[test]
    fn a_distinct_password_produces_an_approved_verifier_without_rendering_secrets() {
        let temporary = b"temporary credential";
        let replacement = b"new ordinary password";
        let current = verifier(temporary);
        let prepared = PreparedPasswordReplacement::prepare(
            &authenticator(),
            current.as_str(),
            PasswordReplacementInput::new(Zeroizing::new(replacement.to_vec())).unwrap(),
        )
        .unwrap();
        assert_eq!(
            format!("{prepared:?}"),
            "PreparedPasswordReplacement(REDACTED)"
        );

        let created = prepared.into_verifier();
        assert_eq!(
            authenticator()
                .authenticate(StoredCredential::Verifier(created.as_str()), replacement)
                .unwrap(),
            PasswordVerdict::Verified { replacement: None }
        );
        let rendered = format!(
            "{:?} {:?}",
            PasswordReplacementInput::new(Zeroizing::new(replacement.to_vec())).unwrap(),
            PasswordReplacementError::SamePassword
        );
        assert!(!rendered.contains(std::str::from_utf8(temporary).unwrap()));
        assert!(!rendered.contains(std::str::from_utf8(replacement).unwrap()));
        assert!(!rendered.contains(created.as_str()));
    }
}
