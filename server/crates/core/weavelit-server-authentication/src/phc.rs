//! Encoding of a PHC string at a known profile.
//!
//! The Server needs one PHC string it can build without running Argon2: the
//! decoy every denial path verifies against. Encoding it through the hashing
//! library's own PHC writer keeps the format authoritative and guarantees the
//! decoy is a real, parseable verifier at the current profile rather than a
//! shape that a rejection could distinguish from a stored one.

use argon2::password_hash::{Output, ParamsString, PasswordHash, SaltString};

use crate::error::AuthenticationError;
use crate::profile::Argon2Profile;

/// Encodes a salt and derived output as a PHC string at `profile`.
///
/// The caller supplies both byte strings; this function contributes only the
/// encoding. It rejects a salt or output whose length does not match the
/// profile, so an encoded value always resolves back to the profile it claims.
pub(crate) fn encode_verifier(
    profile: &Argon2Profile,
    salt: &[u8],
    output: &[u8],
) -> Result<String, AuthenticationError> {
    if salt.len() != profile.salt_bytes() || output.len() != profile.output_bytes() {
        return Err(AuthenticationError::UnsupportedProfile);
    }

    let params = profile.params()?;
    let params = ParamsString::try_from(&params).map_err(|_| AuthenticationError::HashingFailed)?;
    let salt = SaltString::encode_b64(salt).map_err(|_| AuthenticationError::HashingFailed)?;
    let output = Output::new(output).map_err(|_| AuthenticationError::HashingFailed)?;

    Ok(PasswordHash {
        algorithm: profile.algorithm().ident(),
        version: Some(profile.version().into()),
        params,
        salt: Some(salt.as_salt()),
        hash: Some(output),
    }
    .to_string())
}

#[cfg(test)]
mod tests {
    use argon2::password_hash::PasswordHash;

    use super::encode_verifier;
    use crate::error::AuthenticationError;
    use crate::profile::{CURRENT_ARGON2_PROFILE, PasswordPolicy};

    #[test]
    fn an_encoded_verifier_resolves_back_to_the_profile_that_produced_it() {
        let encoded = encode_verifier(&CURRENT_ARGON2_PROFILE, &[7_u8; 16], &[9_u8; 32])
            .expect("the current profile must encode");
        assert_eq!(
            encoded,
            "$argon2id$v=19$m=65536,t=3,p=1$BwcHBwcHBwcHBwcHBwcHBw$CQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQkJCQk"
        );

        let parsed = PasswordHash::new(&encoded).expect("the encoded value must parse");
        assert_eq!(
            PasswordPolicy::approved().resolve(&parsed),
            Some(&CURRENT_ARGON2_PROFILE)
        );
    }

    #[test]
    fn encoding_refuses_a_salt_or_output_that_contradicts_the_profile() {
        assert_eq!(
            encode_verifier(&CURRENT_ARGON2_PROFILE, &[7_u8; 8], &[9_u8; 32]),
            Err(AuthenticationError::UnsupportedProfile)
        );
        assert_eq!(
            encode_verifier(&CURRENT_ARGON2_PROFILE, &[7_u8; 16], &[9_u8; 16]),
            Err(AuthenticationError::UnsupportedProfile)
        );
    }
}
