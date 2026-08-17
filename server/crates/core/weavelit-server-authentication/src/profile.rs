//! The approved Argon2id profile and the closed allowlist of accepted profiles.
//!
//! A stored password verifier is attacker-influenced input. It arrives from the
//! Application Database, and a restored backup can carry whatever encoded
//! parameters its author chose. Argon2 verification allocates the memory the
//! encoded string asks for, so verifying against a stored profile without first
//! checking it lets a single login attempt request an arbitrary allocation.
//!
//! This module closes that by policy rather than by bound. A stored verifier is
//! attempted only when its algorithm, version, cost parameters, salt length, and
//! output length exactly match an entry in an explicitly listed allowlist. Every
//! allowlisted entry is held under [`MAX_VERIFICATION_MEMORY_KIB`], so the
//! memory one verification can request is fixed by the list rather than by the
//! stored value. A verifier outside the list is refused; it is never attempted.

use argon2::password_hash::{PasswordHash, Salt};
use argon2::{Algorithm, Params, Version};

use crate::error::AuthenticationError;

/// The most verification memory any accepted profile may request, in kibibytes.
///
/// This is the approved 64 MiB verification ceiling. It bounds one Argon2
/// verification, which is what a single unauthenticated login attempt can cost.
pub const MAX_VERIFICATION_MEMORY_KIB: u32 = 65_536;

/// The profile every new and rehashed password verifier is produced at.
///
/// Argon2id version 1.3 at `m=65536` KiB, `t=3`, `p=1`, with a 16-byte random
/// salt and a 32-byte output, as recorded in the Authentication Design.
pub const CURRENT_ARGON2_PROFILE: Argon2Profile =
    Argon2Profile::new(Algorithm::Argon2id, Version::V0x13, 65_536, 3, 1, 16, 32);

/// The closed set of profiles a stored verifier may be attempted against.
///
/// This list is the whole policy. Adding an entry is a deliberate, reviewed
/// change that must keep the entry under [`MAX_VERIFICATION_MEMORY_KIB`], and
/// removing an entry immediately stops accepting verifiers stored at it.
pub const ACCEPTED_ARGON2_PROFILES: &[Argon2Profile] = &[CURRENT_ARGON2_PROFILE];

// The approved constants are checked here so an allowlist entry above the
// verification ceiling, or a current profile missing from the list, cannot be
// committed at all.
const _: () = {
    assert!(
        CURRENT_ARGON2_PROFILE.memory_kib <= MAX_VERIFICATION_MEMORY_KIB,
        "the current profile must sit within the approved verification ceiling"
    );

    let mut index = 0;
    let mut current_is_accepted = false;
    while index < ACCEPTED_ARGON2_PROFILES.len() {
        assert!(
            ACCEPTED_ARGON2_PROFILES[index].memory_kib <= MAX_VERIFICATION_MEMORY_KIB,
            "every accepted profile must sit within the approved verification ceiling"
        );
        if ACCEPTED_ARGON2_PROFILES[index].equals(&CURRENT_ARGON2_PROFILE) {
            current_is_accepted = true;
        }
        index += 1;
    }
    assert!(
        current_is_accepted,
        "the current profile must itself be an accepted profile"
    );
};

/// One complete Argon2 password-hashing profile.
///
/// A profile is compared as a whole. There is no notion of a stored value being
/// "strong enough"; it either matches an accepted profile exactly or it does
/// not.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Argon2Profile {
    algorithm: Algorithm,
    version: Version,
    memory_kib: u32,
    iterations: u32,
    lanes: u32,
    salt_bytes: usize,
    output_bytes: usize,
}

impl Argon2Profile {
    /// Describes a profile by its algorithm, version, costs, and lengths.
    #[must_use]
    pub const fn new(
        algorithm: Algorithm,
        version: Version,
        memory_kib: u32,
        iterations: u32,
        lanes: u32,
        salt_bytes: usize,
        output_bytes: usize,
    ) -> Self {
        Self {
            algorithm,
            version,
            memory_kib,
            iterations,
            lanes,
            salt_bytes,
            output_bytes,
        }
    }

    /// Returns the Argon2 variant this profile uses.
    #[must_use]
    pub const fn algorithm(&self) -> Algorithm {
        self.algorithm
    }

    /// Returns the Argon2 version this profile uses.
    #[must_use]
    pub const fn version(&self) -> Version {
        self.version
    }

    /// Returns the memory cost in kibibytes.
    #[must_use]
    pub const fn memory_kib(&self) -> u32 {
        self.memory_kib
    }

    /// Returns the iteration count.
    #[must_use]
    pub const fn iterations(&self) -> u32 {
        self.iterations
    }

    /// Returns the degree of parallelism.
    #[must_use]
    pub const fn lanes(&self) -> u32 {
        self.lanes
    }

    /// Returns the required random-salt length in bytes.
    #[must_use]
    pub const fn salt_bytes(&self) -> usize {
        self.salt_bytes
    }

    /// Returns the required derived-output length in bytes.
    #[must_use]
    pub const fn output_bytes(&self) -> usize {
        self.output_bytes
    }

    /// Reports whether this profile stays within the verification ceiling.
    #[must_use]
    pub const fn within_verification_ceiling(&self) -> bool {
        self.memory_kib <= MAX_VERIFICATION_MEMORY_KIB
    }

    /// Builds the hashing-library parameters for this profile.
    ///
    /// This is the only place a profile becomes Argon2 parameters, so no
    /// parameter set reaches the hashing library without originating in an
    /// accepted profile.
    pub(crate) fn params(&self) -> Result<Params, AuthenticationError> {
        if !self.within_verification_ceiling() {
            return Err(AuthenticationError::UnsupportedProfile);
        }
        // A PHC salt encodes to at most 64 Base64 characters and a PHC output to
        // at most 64 bytes, so a profile outside these lengths could not be
        // encoded at all.
        if !(8..=48).contains(&self.salt_bytes) || !(16..=64).contains(&self.output_bytes) {
            return Err(AuthenticationError::UnsupportedProfile);
        }
        Params::new(
            self.memory_kib,
            self.iterations,
            self.lanes,
            Some(self.output_bytes),
        )
        .map_err(|_| AuthenticationError::UnsupportedProfile)
    }

    /// Reports whether an encoded verifier was produced at exactly this profile.
    ///
    /// Every encoded field is checked: the algorithm identifier, an explicitly
    /// present version, the three cost parameters, the absence of a key
    /// identifier and associated data, the decoded salt length, and the output
    /// length. A parse failure or an unknown parameter name is a mismatch.
    pub(crate) fn matches_encoded(&self, encoded: &PasswordHash<'_>) -> bool {
        let Ok(algorithm) = Algorithm::try_from(encoded.algorithm) else {
            return false;
        };
        if algorithm != self.algorithm {
            return false;
        }

        // An absent version field defaults to the library's version rather than
        // the stored one, so a verifier that omits it is refused outright.
        let Some(version) = encoded.version else {
            return false;
        };
        let Ok(version) = Version::try_from(version) else {
            return false;
        };
        if version != self.version {
            return false;
        }

        let Ok(params) = Params::try_from(encoded) else {
            return false;
        };
        if params.m_cost() != self.memory_kib
            || params.t_cost() != self.iterations
            || params.p_cost() != self.lanes
        {
            return false;
        }
        if !params.keyid().is_empty() || !params.data().is_empty() {
            return false;
        }
        if params.output_len() != Some(self.output_bytes) {
            return false;
        }

        let Some(output) = encoded.hash.as_ref() else {
            return false;
        };
        if output.len() != self.output_bytes {
            return false;
        }

        let Some(salt) = encoded.salt else {
            return false;
        };
        let mut decoded = [0_u8; Salt::MAX_LENGTH];
        let Ok(salt) = salt.decode_b64(&mut decoded) else {
            return false;
        };
        salt.len() == self.salt_bytes
    }

    /// Compares two profiles in a const context.
    const fn equals(&self, other: &Self) -> bool {
        (self.algorithm as u32) == (other.algorithm as u32)
            && (self.version as u32) == (other.version as u32)
            && self.memory_kib == other.memory_kib
            && self.iterations == other.iterations
            && self.lanes == other.lanes
            && self.salt_bytes == other.salt_bytes
            && self.output_bytes == other.output_bytes
    }
}

/// The current profile paired with the closed allowlist it is drawn from.
///
/// A policy is validated once, on construction, so every later verification and
/// rehash works from parameters that were already checked against the approved
/// verification ceiling.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasswordPolicy {
    current: Argon2Profile,
    accepted: &'static [Argon2Profile],
}

impl PasswordPolicy {
    /// Returns the approved profile and the approved closed allowlist.
    #[must_use]
    pub const fn approved() -> Self {
        Self {
            current: CURRENT_ARGON2_PROFILE,
            accepted: ACCEPTED_ARGON2_PROFILES,
        }
    }

    /// Builds a policy from an explicit current profile and allowlist.
    ///
    /// Construction fails when any accepted profile exceeds
    /// [`MAX_VERIFICATION_MEMORY_KIB`], when any accepted profile is not a valid
    /// Argon2 parameter set, or when the current profile is not itself
    /// accepted. A policy therefore cannot exist that would rehash into a
    /// profile it would later refuse.
    pub fn new(
        current: Argon2Profile,
        accepted: &'static [Argon2Profile],
    ) -> Result<Self, AuthenticationError> {
        for profile in accepted {
            profile.params()?;
        }
        current.params()?;
        if !accepted.contains(&current) {
            return Err(AuthenticationError::UnsupportedProfile);
        }
        Ok(Self { current, accepted })
    }

    /// Returns the profile new and rehashed verifiers are produced at.
    #[must_use]
    pub const fn current(&self) -> &Argon2Profile {
        &self.current
    }

    /// Returns the closed allowlist of profiles a stored verifier may use.
    #[must_use]
    pub const fn accepted(&self) -> &'static [Argon2Profile] {
        self.accepted
    }

    /// Returns the accepted profile an encoded verifier was produced at.
    ///
    /// A `None` result means the verifier must not be attempted at all. Restore
    /// content validation asks the same question of a verifier a backup carries,
    /// so this is the one authority both the authentication decision and that
    /// validation resolve a stored verifier against.
    #[must_use]
    pub fn resolve(&self, encoded: &PasswordHash<'_>) -> Option<&'static Argon2Profile> {
        self.accepted
            .iter()
            .find(|profile| profile.matches_encoded(encoded))
    }
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self::approved()
    }
}

#[cfg(test)]
mod tests {
    use argon2::password_hash::PasswordHash;
    use argon2::{Algorithm, Version};

    use super::{
        ACCEPTED_ARGON2_PROFILES, Argon2Profile, CURRENT_ARGON2_PROFILE,
        MAX_VERIFICATION_MEMORY_KIB, PasswordPolicy,
    };
    use crate::error::AuthenticationError;
    use crate::phc::encode_verifier;

    #[test]
    fn the_approved_profile_is_the_documented_argon2id_profile() {
        assert_eq!(CURRENT_ARGON2_PROFILE.algorithm(), Algorithm::Argon2id);
        assert_eq!(CURRENT_ARGON2_PROFILE.version(), Version::V0x13);
        assert_eq!(CURRENT_ARGON2_PROFILE.memory_kib(), 65_536);
        assert_eq!(CURRENT_ARGON2_PROFILE.iterations(), 3);
        assert_eq!(CURRENT_ARGON2_PROFILE.lanes(), 1);
        assert_eq!(CURRENT_ARGON2_PROFILE.salt_bytes(), 16);
        assert_eq!(CURRENT_ARGON2_PROFILE.output_bytes(), 32);
    }

    #[test]
    fn the_allowlist_is_closed_and_bounded_by_the_verification_ceiling() {
        assert_eq!(
            ACCEPTED_ARGON2_PROFILES.len(),
            1,
            "the allowlist starts as exactly the current profile"
        );
        assert_eq!(ACCEPTED_ARGON2_PROFILES[0], CURRENT_ARGON2_PROFILE);
        for profile in ACCEPTED_ARGON2_PROFILES {
            assert!(profile.memory_kib() <= MAX_VERIFICATION_MEMORY_KIB);
        }
        assert_eq!(MAX_VERIFICATION_MEMORY_KIB, 65_536);
    }

    #[test]
    fn the_approved_policy_resolves_a_verifier_at_the_current_profile() {
        let policy = PasswordPolicy::approved();
        let encoded = encode_verifier(&CURRENT_ARGON2_PROFILE, &[7_u8; 16], &[9_u8; 32])
            .expect("the current profile must encode");
        let parsed = PasswordHash::new(&encoded).expect("the fixture must be a valid PHC string");
        assert_eq!(policy.resolve(&parsed), Some(&CURRENT_ARGON2_PROFILE));
    }

    #[test]
    fn a_profile_outside_the_allowlist_never_resolves() {
        let policy = PasswordPolicy::approved();
        // Each entry differs from the approved profile in exactly one field.
        let rejected = [
            // The hostile high-memory verifier a restored backup could carry.
            "$argon2id$v=19$m=4194304,t=100,p=16$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // A different Argon2 variant.
            "$argon2i$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            "$argon2d$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // Argon2 version 1.0.
            "$argon2id$v=16$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // No version field at all.
            "$argon2id$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // Cheaper than the approved profile; still not on the list.
            "$argon2id$v=19$m=32768,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            "$argon2id$v=19$m=65536,t=2,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            "$argon2id$v=19$m=65536,t=3,p=2$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // An eight-byte salt instead of sixteen.
            "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHQ$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // A sixteen-byte output instead of thirty-two.
            "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPg",
            // A key identifier and associated data the Server never issues.
            "$argon2id$v=19$m=65536,t=3,p=1,keyid=YWJj$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            "$argon2id$v=19$m=65536,t=3,p=1,data=YWJj$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // Another password-hashing function entirely.
            "$scrypt$ln=16,r=8,p=1$c2FsdHNhbHRzYWx0c2Fs$Ac2wa9tzcQ1H0T2iQXPfSPk8SmzYJXvI2xEo2Y0N9tQ",
            // No encoded output at all.
            "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0c2Fs",
        ];

        for candidate in rejected {
            let parsed = PasswordHash::new(candidate)
                .unwrap_or_else(|error| panic!("{candidate} must parse as PHC: {error}"));
            assert_eq!(
                policy.resolve(&parsed),
                None,
                "{candidate} must never resolve to an accepted profile"
            );
        }
    }

    #[test]
    fn a_policy_refuses_an_allowlist_above_the_verification_ceiling() {
        static ABOVE_CEILING: [Argon2Profile; 1] = [Argon2Profile::new(
            Algorithm::Argon2id,
            Version::V0x13,
            MAX_VERIFICATION_MEMORY_KIB + 1,
            3,
            1,
            16,
            32,
        )];

        assert_eq!(
            PasswordPolicy::new(ABOVE_CEILING[0], &ABOVE_CEILING),
            Err(AuthenticationError::UnsupportedProfile)
        );
    }

    #[test]
    fn a_policy_refuses_a_current_profile_that_is_not_accepted() {
        static ACCEPTED: [Argon2Profile; 1] = [Argon2Profile::new(
            Algorithm::Argon2id,
            Version::V0x13,
            1_024,
            1,
            1,
            16,
            32,
        )];
        let unlisted = Argon2Profile::new(Algorithm::Argon2id, Version::V0x13, 2_048, 1, 1, 16, 32);

        assert_eq!(
            PasswordPolicy::new(unlisted, &ACCEPTED),
            Err(AuthenticationError::UnsupportedProfile)
        );
        assert!(PasswordPolicy::new(ACCEPTED[0], &ACCEPTED).is_ok());
    }
}
