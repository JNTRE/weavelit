//! Creation of the password verifier a newly created account is stored with.
//!
//! Producing a verifier is the one authentication operation whose caller is not
//! deciding anything: Init has a password and needs the value that is safe to
//! persist. That caller has no business choosing a cost profile or a salt, so
//! neither appears in this module's public surface. The profile is the approved
//! current profile and the salt is freshly generated here, which makes "hash
//! the first Administrator's password more cheaply" something a caller cannot
//! express rather than something it is trusted not to do.

use std::fmt;

use crate::engine::{Argon2Engine, RustCryptoArgon2};
use crate::error::AuthenticationError;
use crate::password::MAX_SALT_BYTES;
use crate::profile::PasswordPolicy;
use crate::random::random_bytes;

/// A password verifier produced at the approved current profile.
///
/// The constructor is private to this crate, so the only way to obtain this
/// value is [`PasswordVerifierFactory::create`]. A caller therefore cannot
/// present a verifier it produced at a profile of its own choosing, and a test
/// double cannot stand in for the real one.
#[derive(Clone, Eq, PartialEq)]
pub struct NewPasswordVerifier(String);

impl NewPasswordVerifier {
    /// Returns the encoded verifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns the encoded verifier.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for NewPasswordVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("NewPasswordVerifier(REDACTED)")
    }
}

/// Produces the password verifier a newly created account is stored with.
///
/// The factory carries the approved policy rather than accepting one, so the
/// closed allowlist in this crate governs every verifier it produces.
#[derive(Clone, Copy, Debug)]
pub struct PasswordVerifierFactory {
    engine: RustCryptoArgon2,
    policy: PasswordPolicy,
}

impl PasswordVerifierFactory {
    /// Creates the factory bound to the approved policy and its engine.
    #[must_use]
    pub fn approved() -> Self {
        let policy = PasswordPolicy::approved();
        Self {
            engine: RustCryptoArgon2::new(policy),
            policy,
        }
    }

    /// Returns the policy every produced verifier is created under.
    #[must_use]
    pub const fn policy(&self) -> &PasswordPolicy {
        &self.policy
    }

    /// Produces a verifier for `password` at the policy's current profile.
    ///
    /// # Errors
    ///
    /// Returns [`AuthenticationError`] when operating-system randomness is
    /// unavailable or the hashing library refuses to produce the encoding. The
    /// error never carries the password or the salt.
    pub fn create(&self, password: &[u8]) -> Result<NewPasswordVerifier, AuthenticationError> {
        let current = self.policy.current();
        let salt = random_bytes::<MAX_SALT_BYTES>()?;
        let salt = salt
            .get(..current.salt_bytes())
            .ok_or(AuthenticationError::UnsupportedProfile)?;

        Ok(NewPasswordVerifier(
            self.engine.hash(password, current, salt)?,
        ))
    }
}

impl Default for PasswordVerifierFactory {
    fn default() -> Self {
        Self::approved()
    }
}

#[cfg(test)]
mod tests {
    use argon2::password_hash::PasswordHash;

    use super::{NewPasswordVerifier, PasswordVerifierFactory};
    use crate::engine::{Argon2Engine as _, RustCryptoArgon2};
    use crate::profile::{CURRENT_ARGON2_PROFILE, PasswordPolicy};

    const PASSWORD: &[u8] = b"first administrator password";

    #[test]
    fn a_created_verifier_is_produced_at_the_current_approved_profile() {
        let created = PasswordVerifierFactory::approved()
            .create(PASSWORD)
            .expect("the approved factory must produce a verifier");

        let parsed =
            PasswordHash::new(created.as_str()).expect("the verifier must be a PHC string");
        assert_eq!(
            PasswordPolicy::approved().resolve(&parsed),
            Some(&CURRENT_ARGON2_PROFILE),
            "a created verifier must resolve to the current approved profile"
        );
    }

    #[test]
    fn a_created_verifier_authenticates_only_the_password_it_was_created_for() {
        let created = PasswordVerifierFactory::approved()
            .create(PASSWORD)
            .expect("the approved factory must produce a verifier");
        let engine = RustCryptoArgon2::default();

        assert!(engine.verify(PASSWORD, &CURRENT_ARGON2_PROFILE, created.as_str()));
        assert!(!engine.verify(b"wrong password", &CURRENT_ARGON2_PROFILE, created.as_str()));
    }

    #[test]
    fn two_verifiers_for_one_password_differ_because_each_salt_is_fresh() {
        let factory = PasswordVerifierFactory::approved();
        let first = factory
            .create(PASSWORD)
            .expect("the first must be produced");
        let second = factory
            .create(PASSWORD)
            .expect("the second must be produced");

        assert_ne!(
            first.as_str(),
            second.as_str(),
            "a caller-invisible fresh salt must make repeated verifiers differ"
        );
    }

    #[test]
    fn a_created_verifier_is_redacted_in_debug_output() {
        let created = PasswordVerifierFactory::approved()
            .create(PASSWORD)
            .expect("the approved factory must produce a verifier");

        // The needle is the verifier itself, so this cannot pass vacuously: the
        // value is present in `as_str` and absent from the rendering.
        let rendered = format!("{created:?}");
        assert!(!created.as_str().is_empty());
        assert!(!rendered.contains(created.as_str()));
        assert!(!rendered.contains('$'), "no PHC text may reach a rendering");
        assert_eq!(rendered, "NewPasswordVerifier(REDACTED)");
    }

    #[test]
    fn the_factory_debug_output_carries_no_verifier_material() {
        let factory = PasswordVerifierFactory::approved();
        let created = factory
            .create(PASSWORD)
            .expect("a verifier must be produced");

        let rendered = format!("{factory:?}");
        assert!(!rendered.contains(created.as_str()));
        assert_eq!(factory.policy().current(), &CURRENT_ARGON2_PROFILE);
    }

    #[test]
    fn the_encoded_verifier_survives_being_consumed() {
        let created = PasswordVerifierFactory::approved()
            .create(PASSWORD)
            .expect("the approved factory must produce a verifier");
        let encoded = created.clone().into_string();

        assert_eq!(encoded, created.as_str());
        assert_eq!(created, NewPasswordVerifier(encoded));
    }
}
