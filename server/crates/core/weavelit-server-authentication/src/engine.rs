//! The Argon2 execution seam and its RustCrypto implementation.
//!
//! Password verification is separated from the decision logic behind
//! [`Argon2Engine`] so the equal-work property of every denial path can be
//! observed directly, by counting the verification operations a decision
//! performs, instead of being inferred from elapsed time.

use argon2::Argon2;
use argon2::password_hash::{PasswordHash, PasswordHasher as _, PasswordVerifier as _, SaltString};

use crate::error::AuthenticationError;
use crate::profile::{Argon2Profile, PasswordPolicy};

/// Executes the Argon2 work an authentication decision requires.
///
/// An implementation is never given a policy decision to make. It receives an
/// encoded verifier that the caller has already resolved to `profile`, and it
/// refuses anything that does not still match that profile.
pub trait Argon2Engine {
    /// Verifies `password` against `encoded`, which was produced at `profile`.
    ///
    /// This reports only whether the password matched. It is not an error
    /// channel: an unusable encoded value is a non-match, so no caller can tell
    /// a malformed stored verifier from a wrong password.
    fn verify(&self, password: &[u8], profile: &Argon2Profile, encoded: &str) -> bool;

    /// Produces a new encoded verifier for `password` at `profile`.
    fn hash(
        &self,
        password: &[u8],
        profile: &Argon2Profile,
        salt: &[u8],
    ) -> Result<String, AuthenticationError>;
}

/// The RustCrypto `argon2` implementation of the engine.
///
/// The engine carries the policy so it can re-check, immediately before running
/// Argon2, that the encoded verifier still matches an accepted profile. The
/// hashing library derives its cost parameters from the encoded string it is
/// given, so this re-check is what keeps a stored value from selecting the
/// memory a verification allocates.
#[derive(Clone, Copy, Debug)]
pub struct RustCryptoArgon2 {
    policy: PasswordPolicy,
}

impl RustCryptoArgon2 {
    /// Creates an engine bound to `policy`.
    #[must_use]
    pub const fn new(policy: PasswordPolicy) -> Self {
        Self { policy }
    }
}

impl Default for RustCryptoArgon2 {
    fn default() -> Self {
        Self::new(PasswordPolicy::approved())
    }
}

impl Argon2Engine for RustCryptoArgon2 {
    fn verify(&self, password: &[u8], profile: &Argon2Profile, encoded: &str) -> bool {
        let Ok(parsed) = PasswordHash::new(encoded) else {
            return false;
        };
        // Defence in depth: the caller resolved this verifier already, and the
        // engine refuses to run unless it resolves to the same accepted profile
        // here too.
        if self.policy.resolve(&parsed) != Some(profile) {
            return false;
        }
        let Ok(params) = profile.params() else {
            return false;
        };

        Argon2::new(profile.algorithm(), profile.version(), params)
            .verify_password(password, &parsed)
            .is_ok()
    }

    fn hash(
        &self,
        password: &[u8],
        profile: &Argon2Profile,
        salt: &[u8],
    ) -> Result<String, AuthenticationError> {
        if salt.len() != profile.salt_bytes() {
            return Err(AuthenticationError::UnsupportedProfile);
        }
        let params = profile.params()?;
        let salt = SaltString::encode_b64(salt).map_err(|_| AuthenticationError::HashingFailed)?;

        Argon2::new(profile.algorithm(), profile.version(), params)
            .hash_password(password, &salt)
            .map(|hash| hash.to_string())
            .map_err(|_| AuthenticationError::HashingFailed)
    }
}

#[cfg(test)]
mod tests {
    use argon2::password_hash::PasswordHash;
    use argon2::{Algorithm, Version};

    use super::{Argon2Engine, RustCryptoArgon2};
    use crate::error::AuthenticationError;
    use crate::profile::{Argon2Profile, CURRENT_ARGON2_PROFILE, PasswordPolicy};

    /// A deliberately cheap profile so the real Argon2 can run inside a test.
    static CHEAP_PROFILES: [Argon2Profile; 1] = [Argon2Profile::new(
        Algorithm::Argon2id,
        Version::V0x13,
        64,
        1,
        1,
        16,
        32,
    )];

    fn cheap_policy() -> PasswordPolicy {
        PasswordPolicy::new(CHEAP_PROFILES[0], &CHEAP_PROFILES)
            .expect("the cheap test profile must be a valid policy")
    }

    #[test]
    fn the_engine_verifies_only_the_password_it_hashed() {
        let policy = cheap_policy();
        let engine = RustCryptoArgon2::new(policy);
        let encoded = engine
            .hash(b"correct horse", policy.current(), &[3_u8; 16])
            .expect("hashing must succeed");

        let parsed = PasswordHash::new(&encoded).expect("the produced value must parse");
        assert_eq!(policy.resolve(&parsed), Some(policy.current()));
        assert!(engine.verify(b"correct horse", policy.current(), &encoded));
        assert!(!engine.verify(b"correct hors", policy.current(), &encoded));
        assert!(!engine.verify(b"", policy.current(), &encoded));
    }

    #[test]
    fn the_engine_refuses_a_verifier_outside_its_own_policy() {
        let engine = RustCryptoArgon2::new(cheap_policy());
        // Encoded at the approved profile, which this engine's policy does not
        // accept. Refusal happens before Argon2 runs, so no 64 MiB allocation is
        // requested.
        let approved =
            crate::phc::encode_verifier(&CURRENT_ARGON2_PROFILE, &[1_u8; 16], &[2_u8; 32])
                .expect("the approved profile must encode");
        assert!(!engine.verify(b"anything", &CURRENT_ARGON2_PROFILE, &approved));
        assert!(!engine.verify(
            b"anything",
            CHEAP_PROFILES.first().expect("one entry"),
            &approved
        ));
    }

    #[test]
    fn the_engine_refuses_a_hostile_high_memory_verifier_without_running_it() {
        let engine = RustCryptoArgon2::default();
        let hostile = "$argon2id$v=19$m=4194304,t=100,p=16$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ";
        assert!(!engine.verify(b"anything", &CURRENT_ARGON2_PROFILE, hostile));
        assert!(!engine.verify(b"anything", &CURRENT_ARGON2_PROFILE, "not a phc string"));
    }

    #[test]
    fn hashing_refuses_a_salt_that_contradicts_the_profile() {
        let policy = cheap_policy();
        let engine = RustCryptoArgon2::new(policy);
        assert_eq!(
            engine.hash(b"password", policy.current(), &[3_u8; 8]),
            Err(AuthenticationError::UnsupportedProfile)
        );
    }
}
