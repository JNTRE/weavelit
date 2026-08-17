//! Bounded secret values carried by an Init request.
//!
//! Two properties are structural rather than conventional. A secret clears on
//! drop and redacts in `Debug`, so it cannot survive its request or reach a
//! rendering. And reading one requires a borrow of [`AuthorizedInit`], whose
//! constructor is private to this crate and whose only producer consults the
//! lifecycle authority; an Init step that reads a submitted secret before that
//! answer has no value to pass and does not compile.

use std::fmt;

use weavelit_server_lifecycle::MAX_PROTECTED_PLAINTEXT_BYTES;
use zeroize::Zeroizing;

use crate::{AuthorizedInit, RequestError};

/// Maximum UTF-8 bytes accepted in a submitted initial password.
///
/// Argon2 costs the same for every accepted length, so this bounds what one
/// request may hold in memory rather than expressing a strength policy. The
/// approved profile decides the work; this decides the size.
pub const MAX_PASSWORD_BYTES: usize = 1024;

/// Maximum bytes accepted in one submitted protected Log Module setting.
///
/// A submitted secret is sealed under the deployment's at-rest key, so it is
/// bounded by what that key's envelope accepts rather than by the larger bound
/// on an already-sealed stored value.
pub const MAX_PROTECTED_SETTING_BYTES: usize = MAX_PROTECTED_PLAINTEXT_BYTES;

/// The submitted password for the first local Human User.
///
/// The value is cleared on drop and never enters a display, debug, log, or
/// client-visible representation.
#[derive(Clone, Eq, PartialEq)]
pub struct InitialPassword(Zeroizing<String>);

impl InitialPassword {
    /// Creates a bounded submitted password.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError`] when the value is empty or exceeds
    /// [`MAX_PASSWORD_BYTES`]. The error never carries the value.
    pub fn new(value: String) -> Result<Self, RequestError> {
        if value.is_empty() {
            return Err(RequestError::SecretEmpty);
        }
        if value.len() > MAX_PASSWORD_BYTES {
            return Err(RequestError::SecretTooLong);
        }

        Ok(Self(Zeroizing::new(value)))
    }

    /// Returns the submitted password for verifier creation.
    ///
    /// The [`AuthorizedInit`] borrow is the compile-time evidence that the
    /// lifecycle authority already answered. It is deliberately unused at run
    /// time: the ordering it enforces is checked by the type system, not by a
    /// branch that could be removed.
    pub(crate) fn expose(&self, _authorized: &AuthorizedInit) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the byte length of the submitted password.
    ///
    /// The length is not the secret and is needed to reason about bounds in
    /// tests and diagnostics, so it is readable without the authority borrow.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the submitted password is empty.
    ///
    /// A constructed value never is; this exists so the length accessor reads
    /// idiomatically alongside it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for InitialPassword {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitialPassword(REDACTED)")
    }
}

/// A submitted Log Module secret awaiting at-rest protection.
///
/// The value is cleared on drop and never enters a display, debug, log, or
/// client-visible representation.
#[derive(Clone, Eq, PartialEq)]
pub struct InitialSecret(Zeroizing<Vec<u8>>);

impl InitialSecret {
    /// Creates a bounded submitted secret.
    ///
    /// # Errors
    ///
    /// Returns [`RequestError`] when the value is empty or exceeds
    /// [`MAX_PROTECTED_SETTING_BYTES`]. The error never carries the value.
    pub fn new(value: Vec<u8>) -> Result<Self, RequestError> {
        if value.is_empty() {
            return Err(RequestError::SecretEmpty);
        }
        if value.len() > MAX_PROTECTED_SETTING_BYTES {
            return Err(RequestError::SecretTooLong);
        }

        Ok(Self(Zeroizing::new(value)))
    }

    /// Returns the submitted secret for sealing under the deployment key.
    pub(crate) fn expose(&self, _authorized: &AuthorizedInit) -> &[u8] {
        &self.0
    }

    /// Returns the byte length of the submitted secret.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the submitted secret is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for InitialSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitialSecret(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::{InitialPassword, InitialSecret, MAX_PASSWORD_BYTES, MAX_PROTECTED_SETTING_BYTES};
    use crate::RequestError;

    const PASSWORD: &str = "correct horse battery staple";

    #[test]
    fn a_password_accepts_one_byte_through_its_exact_bound() {
        assert!(InitialPassword::new("a".to_owned()).is_ok());
        assert_eq!(
            InitialPassword::new("a".repeat(MAX_PASSWORD_BYTES))
                .expect("the exact bound must be accepted")
                .len(),
            MAX_PASSWORD_BYTES
        );
    }

    #[test]
    fn a_password_rejects_an_empty_or_overlong_value_without_carrying_it() {
        assert_eq!(
            InitialPassword::new(String::new()),
            Err(RequestError::SecretEmpty)
        );

        let overlong = format!("{PASSWORD}{}", "a".repeat(MAX_PASSWORD_BYTES));
        let error =
            InitialPassword::new(overlong).expect_err("a value past the bound must be rejected");
        assert_eq!(error, RequestError::SecretTooLong);
        assert!(!format!("{error} {error:?}").contains(PASSWORD));
    }

    #[test]
    fn a_password_is_redacted_in_debug_output() {
        let password =
            InitialPassword::new(PASSWORD.to_owned()).expect("the fixture password is valid");

        // The needle is the submitted value, which is present by construction,
        // so this assertion cannot pass because the value was never there.
        let rendered = format!("{password:?}");
        assert_eq!(password.len(), PASSWORD.len());
        assert!(!rendered.contains(PASSWORD));
        assert!(!rendered.contains("horse"));
        assert_eq!(rendered, "InitialPassword(REDACTED)");
        assert!(!password.is_empty());
    }

    #[test]
    fn a_protected_setting_accepts_one_byte_through_its_exact_bound() {
        assert!(InitialSecret::new(vec![7]).is_ok());
        assert_eq!(
            InitialSecret::new(vec![7; MAX_PROTECTED_SETTING_BYTES])
                .expect("the exact bound must be accepted")
                .len(),
            MAX_PROTECTED_SETTING_BYTES
        );
    }

    #[test]
    fn a_protected_setting_rejects_an_empty_or_overlong_value() {
        assert_eq!(
            InitialSecret::new(Vec::new()),
            Err(RequestError::SecretEmpty)
        );
        assert_eq!(
            InitialSecret::new(vec![7; MAX_PROTECTED_SETTING_BYTES + 1]),
            Err(RequestError::SecretTooLong)
        );
    }

    #[test]
    fn a_protected_setting_is_redacted_in_debug_output() {
        let secret = InitialSecret::new(b"log-module-api-token".to_vec())
            .expect("the fixture secret is valid");

        let rendered = format!("{secret:?}");
        assert_eq!(secret.len(), "log-module-api-token".len());
        assert!(!rendered.contains("log-module-api-token"));
        assert!(!rendered.contains("token"));
        assert_eq!(rendered, "InitialSecret(REDACTED)");
        assert!(!secret.is_empty());
    }
}
