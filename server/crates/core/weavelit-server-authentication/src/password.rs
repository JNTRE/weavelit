//! The local password authentication decision.
//!
//! Every denial performs the same Argon2 work. An unknown account, an inactive
//! account, an account with no stored verifier, and an account whose stored
//! verifier is outside the closed allowlist are all verified against a decoy
//! verifier built at the current profile, so the decision costs the same as a
//! wrong password and reveals nothing about which of those it was.

use std::fmt;

use argon2::password_hash::PasswordHash;

use crate::engine::Argon2Engine;
use crate::error::AuthenticationError;
use crate::phc::encode_verifier;
use crate::profile::{Argon2Profile, PasswordPolicy};
use crate::random::random_bytes;

/// The largest random salt any profile may request, in bytes.
///
/// PHC salts are Base64-encoded into at most 64 characters, which decodes to 48
/// bytes.
pub(crate) const MAX_SALT_BYTES: usize = 48;

/// What the caller found in the Application Database for a submitted account.
///
/// The caller performs the lookup; this crate never reads storage. Each variant
/// is a distinct outcome for the caller and an identical amount of work here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoredCredential<'a> {
    /// No account matched the submitted name.
    UnknownAccount,
    /// An account matched but is not active.
    InactiveAccount,
    /// An active account matched but carries no password verifier.
    NoVerifier,
    /// An active account matched and carries this encoded verifier.
    Verifier(&'a str),
}

/// A replacement verifier the caller should persist for the account.
///
/// Produced only after a successful verification against an accepted profile
/// that is not the current one. This crate does not persist it.
#[derive(Clone, Eq, PartialEq)]
pub struct ReplacementVerifier(String);

impl ReplacementVerifier {
    /// Returns the encoded replacement verifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the value and returns the encoded replacement verifier.
    #[must_use]
    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Debug for ReplacementVerifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReplacementVerifier(redacted)")
    }
}

/// The result of one password authentication decision.
///
/// A denial is deliberately not an error, so a caller cannot separate "no such
/// account" from "wrong password" by inspecting an error value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PasswordVerdict {
    /// The password matched a verifier stored at an accepted profile.
    Verified {
        /// Set when the stored verifier was accepted but not at the current
        /// profile, and the caller should replace it with this value.
        replacement: Option<ReplacementVerifier>,
    },
    /// The password was not accepted, for a reason the caller is not told.
    Denied,
}

/// Decides local password authentication under a closed profile allowlist.
pub struct PasswordAuthenticator<E> {
    engine: E,
    policy: PasswordPolicy,
    decoy: String,
}

impl<E: Argon2Engine> PasswordAuthenticator<E> {
    /// Creates an authenticator for `policy` and builds its decoy verifier.
    ///
    /// The decoy is a real PHC string at the policy's current profile with a
    /// random salt and a random output. No password can match it, and it is
    /// built without running Argon2 so construction stays cheap.
    pub fn new(engine: E, policy: PasswordPolicy) -> Result<Self, AuthenticationError> {
        let profile = *policy.current();
        let salt = random_bytes::<MAX_SALT_BYTES>()?;
        let output = random_bytes::<64>()?;
        let decoy = encode_verifier(
            &profile,
            salt.get(..profile.salt_bytes())
                .ok_or(AuthenticationError::UnsupportedProfile)?,
            output
                .get(..profile.output_bytes())
                .ok_or(AuthenticationError::UnsupportedProfile)?,
        )?;

        Ok(Self {
            engine,
            policy,
            decoy,
        })
    }

    /// Returns the policy this authenticator enforces.
    #[must_use]
    pub const fn policy(&self) -> &PasswordPolicy {
        &self.policy
    }

    /// Decides whether `password` authenticates against `credential`.
    ///
    /// Exactly one Argon2 verification runs on every path, including every
    /// denial. An `Err` reports an operational failure such as unavailable
    /// randomness while producing a replacement verifier; it never reports an
    /// authentication decision.
    pub fn authenticate(
        &self,
        credential: StoredCredential<'_>,
        password: &[u8],
    ) -> Result<PasswordVerdict, AuthenticationError> {
        let accepted = self.accepted_verifier(credential);
        let (profile, encoded) = accepted.unwrap_or((self.policy.current(), self.decoy.as_str()));

        let matched = self.engine.verify(password, profile, encoded);

        let Some((profile, _)) = accepted else {
            return Ok(PasswordVerdict::Denied);
        };
        if !matched {
            return Ok(PasswordVerdict::Denied);
        }

        let current = self.policy.current();
        let replacement = if profile == current {
            None
        } else {
            let salt = random_bytes::<MAX_SALT_BYTES>()?;
            let salt = salt
                .get(..current.salt_bytes())
                .ok_or(AuthenticationError::UnsupportedProfile)?;
            Some(ReplacementVerifier(
                self.engine.hash(password, current, salt)?,
            ))
        };

        Ok(PasswordVerdict::Verified { replacement })
    }

    /// Returns the stored verifier and its profile when it may be attempted.
    fn accepted_verifier<'a>(
        &self,
        credential: StoredCredential<'a>,
    ) -> Option<(&'static Argon2Profile, &'a str)> {
        let StoredCredential::Verifier(encoded) = credential else {
            return None;
        };
        let parsed = PasswordHash::new(encoded).ok()?;
        self.policy
            .resolve(&parsed)
            .map(|profile| (profile, encoded))
    }
}

impl<E> fmt::Debug for PasswordAuthenticator<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PasswordAuthenticator")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use argon2::password_hash::PasswordHash;
    use argon2::{Algorithm, Params, Version};

    use super::{PasswordAuthenticator, PasswordVerdict, ReplacementVerifier, StoredCredential};
    use crate::engine::{Argon2Engine, RustCryptoArgon2};
    use crate::error::AuthenticationError;
    use crate::phc::encode_verifier;
    use crate::profile::{Argon2Profile, CURRENT_ARGON2_PROFILE, PasswordPolicy};

    /// The observable shape of one Argon2 verification.
    ///
    /// This records what the engine was actually asked to compute, including
    /// the parameters encoded in the verifier it was handed, so a denial that
    /// silently used different work is visible as a different value.
    #[derive(Clone, Debug, Eq, PartialEq)]
    struct VerificationShape {
        requested: Argon2Profile,
        encoded_algorithm: String,
        encoded_version: Option<u32>,
        encoded_memory_kib: u32,
        encoded_iterations: u32,
        encoded_lanes: u32,
        encoded_output_bytes: Option<usize>,
    }

    impl VerificationShape {
        fn observe(profile: &Argon2Profile, encoded: &str) -> Self {
            let parsed =
                PasswordHash::new(encoded).expect("the engine must only see valid PHC strings");
            let params = Params::try_from(&parsed).expect("the parameters must decode");
            Self {
                requested: *profile,
                encoded_algorithm: parsed.algorithm.as_str().to_owned(),
                encoded_version: parsed.version,
                encoded_memory_kib: params.m_cost(),
                encoded_iterations: params.t_cost(),
                encoded_lanes: params.p_cost(),
                encoded_output_bytes: params.output_len(),
            }
        }
    }

    /// An engine that records every operation instead of running Argon2.
    ///
    /// Counting operations proves the equal-work property directly. A
    /// wall-clock comparison would prove it only statistically and would be
    /// flaky under load.
    struct CountingEngine {
        verifications: RefCell<Vec<VerificationShape>>,
        hashes: RefCell<Vec<Argon2Profile>>,
        accept: Option<String>,
    }

    impl CountingEngine {
        fn new() -> Self {
            Self {
                verifications: RefCell::new(Vec::new()),
                hashes: RefCell::new(Vec::new()),
                accept: None,
            }
        }

        /// Accepts exactly one encoded verifier, so a success path is reachable.
        fn accepting(encoded: &str) -> Self {
            Self {
                verifications: RefCell::new(Vec::new()),
                hashes: RefCell::new(Vec::new()),
                accept: Some(encoded.to_owned()),
            }
        }

        fn verifications(&self) -> Vec<VerificationShape> {
            self.verifications.borrow().clone()
        }

        fn hashes(&self) -> Vec<Argon2Profile> {
            self.hashes.borrow().clone()
        }
    }

    impl Argon2Engine for CountingEngine {
        fn verify(&self, _password: &[u8], profile: &Argon2Profile, encoded: &str) -> bool {
            self.verifications
                .borrow_mut()
                .push(VerificationShape::observe(profile, encoded));
            self.accept.as_deref() == Some(encoded)
        }

        fn hash(
            &self,
            _password: &[u8],
            profile: &Argon2Profile,
            salt: &[u8],
        ) -> Result<String, AuthenticationError> {
            self.hashes.borrow_mut().push(*profile);
            encode_verifier(profile, salt, &[0x5a_u8; 32])
        }
    }

    static LEGACY_AND_CURRENT: [Argon2Profile; 2] = [
        Argon2Profile::new(Algorithm::Argon2id, Version::V0x13, 64, 1, 1, 16, 32),
        Argon2Profile::new(Algorithm::Argon2id, Version::V0x13, 128, 2, 1, 16, 32),
    ];

    fn drift_policy() -> PasswordPolicy {
        PasswordPolicy::new(LEGACY_AND_CURRENT[1], &LEGACY_AND_CURRENT)
            .expect("the two-profile test policy must be valid")
    }

    fn approved_verifier(salt: u8) -> String {
        encode_verifier(&CURRENT_ARGON2_PROFILE, &[salt; 16], &[0x11_u8; 32])
            .expect("the approved profile must encode")
    }

    /// A verifier a hostile backup could carry: about 4 GiB of Argon2 memory.
    const HOSTILE_VERIFIER: &str = "$argon2id$v=19$m=4194304,t=100,p=16$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ";

    #[test]
    fn every_denial_path_performs_the_same_verification_work() {
        let stored = approved_verifier(1);
        let denials = [
            ("unknown account", StoredCredential::UnknownAccount),
            ("inactive account", StoredCredential::InactiveAccount),
            ("no stored verifier", StoredCredential::NoVerifier),
            (
                "verifier outside the allowlist",
                StoredCredential::Verifier(HOSTILE_VERIFIER),
            ),
            (
                "unparseable verifier",
                StoredCredential::Verifier("not a phc string"),
            ),
            (
                "wrong password against a stored verifier",
                StoredCredential::Verifier(stored.as_str()),
            ),
        ];

        let mut observed: Vec<(&str, Vec<VerificationShape>)> = Vec::new();
        for (label, credential) in denials {
            let engine = CountingEngine::new();
            let authenticator =
                PasswordAuthenticator::new(engine, PasswordPolicy::approved()).expect("policy");

            assert_eq!(
                authenticator
                    .authenticate(credential, b"submitted password")
                    .expect("a denial is never an error"),
                PasswordVerdict::Denied,
                "{label} must be denied"
            );

            let engine = &authenticator.engine;
            assert!(
                engine.hashes().is_empty(),
                "{label} must not produce a verifier"
            );
            observed.push((label, engine.verifications()));
        }

        let (first_label, expected) = observed.first().expect("at least one denial").clone();
        assert_eq!(expected.len(), 1, "{first_label} must verify exactly once");
        assert_eq!(expected[0].encoded_memory_kib, 65_536);
        assert_eq!(expected[0].encoded_iterations, 3);
        assert_eq!(expected[0].encoded_lanes, 1);
        assert_eq!(expected[0].encoded_algorithm, "argon2id");
        assert_eq!(expected[0].encoded_version, Some(19));
        assert_eq!(expected[0].requested, CURRENT_ARGON2_PROFILE);

        for (label, shapes) in &observed {
            assert_eq!(
                shapes, &expected,
                "{label} must perform the same verification work as {first_label}"
            );
        }
    }

    #[test]
    fn a_hostile_verifier_is_never_handed_to_argon2() {
        let engine = CountingEngine::new();
        let authenticator =
            PasswordAuthenticator::new(engine, PasswordPolicy::approved()).expect("policy");

        assert_eq!(
            authenticator
                .authenticate(
                    StoredCredential::Verifier(HOSTILE_VERIFIER),
                    b"submitted password"
                )
                .expect("a denial is never an error"),
            PasswordVerdict::Denied
        );

        let verifications = authenticator.engine.verifications();
        assert_eq!(verifications.len(), 1);
        assert_eq!(verifications[0].encoded_memory_kib, 65_536);
        assert_eq!(verifications[0].encoded_iterations, 3);
        assert_eq!(verifications[0].encoded_lanes, 1);
        assert_ne!(verifications[0].encoded_memory_kib, 4_194_304);
    }

    #[test]
    fn the_decoy_is_a_distinct_current_profile_verifier_for_each_authenticator() {
        let first = PasswordAuthenticator::new(CountingEngine::new(), PasswordPolicy::approved())
            .expect("policy");
        let second = PasswordAuthenticator::new(CountingEngine::new(), PasswordPolicy::approved())
            .expect("policy");
        assert_ne!(first.decoy, second.decoy, "each decoy must be random");

        let parsed = PasswordHash::new(&first.decoy).expect("the decoy must be a valid PHC string");
        assert_eq!(
            PasswordPolicy::approved().resolve(&parsed),
            Some(&CURRENT_ARGON2_PROFILE),
            "the decoy must resolve to the current profile"
        );

        // No password matches the decoy, even with a real Argon2 engine.
        let real =
            PasswordAuthenticator::new(RustCryptoArgon2::new(drift_policy()), drift_policy())
                .expect("policy");
        let decoy = real.decoy.clone();
        assert_eq!(
            real.authenticate(StoredCredential::Verifier(decoy.as_str()), b"")
                .expect("a denial is never an error"),
            PasswordVerdict::Denied
        );
    }

    #[test]
    fn a_current_profile_verifier_authenticates_without_a_replacement() {
        let policy = drift_policy();
        let engine = RustCryptoArgon2::new(policy);
        let stored = engine
            .hash(b"correct horse", policy.current(), &[4_u8; 16])
            .expect("hashing must succeed");
        let authenticator = PasswordAuthenticator::new(engine, policy).expect("policy");

        assert_eq!(
            authenticator
                .authenticate(StoredCredential::Verifier(&stored), b"correct horse")
                .expect("verification must not fail operationally"),
            PasswordVerdict::Verified { replacement: None }
        );
        assert_eq!(
            authenticator
                .authenticate(StoredCredential::Verifier(&stored), b"wrong horse")
                .expect("a denial is never an error"),
            PasswordVerdict::Denied
        );
    }

    #[test]
    fn an_accepted_legacy_verifier_is_rehashed_at_the_current_profile() {
        let policy = drift_policy();
        let legacy_profile = LEGACY_AND_CURRENT[0];
        assert_ne!(&legacy_profile, policy.current());

        let engine = RustCryptoArgon2::new(policy);
        let stored = engine
            .hash(b"correct horse", &legacy_profile, &[5_u8; 16])
            .expect("hashing must succeed");
        let authenticator = PasswordAuthenticator::new(engine, policy).expect("policy");

        let PasswordVerdict::Verified {
            replacement: Some(replacement),
        } = authenticator
            .authenticate(StoredCredential::Verifier(&stored), b"correct horse")
            .expect("verification must not fail operationally")
        else {
            panic!("an accepted legacy verifier must produce a replacement");
        };

        assert_ne!(replacement.as_str(), stored);
        let parsed =
            PasswordHash::new(replacement.as_str()).expect("the replacement must be valid PHC");
        assert_eq!(policy.resolve(&parsed), Some(policy.current()));

        // The replacement authenticates the same password and needs no further
        // rehash, so the drift is actually resolved.
        assert_eq!(
            authenticator
                .authenticate(
                    StoredCredential::Verifier(replacement.as_str()),
                    b"correct horse"
                )
                .expect("verification must not fail operationally"),
            PasswordVerdict::Verified { replacement: None }
        );
    }

    #[test]
    fn a_denial_never_produces_a_replacement_verifier() {
        let policy = drift_policy();
        let stored = RustCryptoArgon2::new(policy)
            .hash(b"correct horse", &LEGACY_AND_CURRENT[0], &[6_u8; 16])
            .expect("hashing must succeed");
        let authenticator =
            PasswordAuthenticator::new(CountingEngine::new(), policy).expect("policy");

        assert_eq!(
            authenticator
                .authenticate(StoredCredential::Verifier(&stored), b"wrong horse")
                .expect("a denial is never an error"),
            PasswordVerdict::Denied
        );
        assert!(authenticator.engine.hashes().is_empty());
    }

    #[test]
    fn a_rehash_runs_only_at_the_current_profile() {
        let policy = drift_policy();
        let stored = RustCryptoArgon2::new(policy)
            .hash(b"correct horse", &LEGACY_AND_CURRENT[0], &[7_u8; 16])
            .expect("hashing must succeed");
        let authenticator =
            PasswordAuthenticator::new(CountingEngine::accepting(&stored), policy).expect("policy");

        assert!(matches!(
            authenticator
                .authenticate(StoredCredential::Verifier(&stored), b"correct horse")
                .expect("verification must not fail operationally"),
            PasswordVerdict::Verified {
                replacement: Some(_)
            }
        ));
        assert_eq!(authenticator.engine.hashes(), vec![*policy.current()]);
    }

    #[test]
    fn a_replacement_verifier_never_renders_its_value() {
        let replacement = ReplacementVerifier(approved_verifier(2));
        let rendered = format!("{replacement:?}");
        assert_eq!(rendered, "ReplacementVerifier(redacted)");
        assert!(!rendered.contains("argon2id"));
    }

    #[test]
    fn the_authenticator_never_renders_its_decoy() {
        let authenticator =
            PasswordAuthenticator::new(CountingEngine::new(), PasswordPolicy::approved())
                .expect("policy");
        let rendered = format!("{authenticator:?}");
        assert!(!rendered.contains(authenticator.decoy.as_str()));
        assert!(rendered.starts_with("PasswordAuthenticator {"));
    }
}
