//! The committed fixtures carry a usable administrator password verifier.
//!
//! A restored fixture is the only way a deployment in this repository gains an
//! account, so an end-to-end sign-in test is only meaningful if the fixture's
//! stored verifier is one the Server would actually accept and one a known
//! password actually verifies against. A placeholder verifier satisfies every
//! structural check the Restore content rules apply and still makes login
//! permanently impossible, so it is checked here against the real
//! authentication decision rather than against its own shape.

mod support;

use argon2::password_hash::{PasswordHash, Salt};
use argon2::{Algorithm, Params, Version};
use support::{FIXTURE_ADMINISTRATOR_PASSWORD, committed, committed_text, validate};
use weavelit_server_authentication::{
    CURRENT_ARGON2_PROFILE, PasswordAuthenticator, PasswordPolicy, PasswordVerdict,
    RustCryptoArgon2, StoredCredential,
};

/// Every committed artifact whose restored state carries the administrator.
const VALID_ARTIFACTS: [&str; 2] = ["valid.wlitbackup", "valid-web-ui-sqlite.wlitbackup"];

/// Returns the encoded verifier a committed artifact restores, read through the
/// production reader rather than from the plaintext expectation beside it.
fn restored_verifier(artifact: &str) -> String {
    let validated = validate(&committed(artifact), &committed_text("valid-identity.txt"))
        .expect("the committed artifact is valid");
    let verifiers = validated.backup().password_verifiers();
    assert_eq!(verifiers.len(), 1, "{artifact}");
    verifiers[0].verifier.as_str().to_owned()
}

fn authenticator() -> PasswordAuthenticator<RustCryptoArgon2> {
    let policy = PasswordPolicy::approved();
    PasswordAuthenticator::new(RustCryptoArgon2::new(policy), policy)
        .expect("the approved policy must build an authenticator")
}

#[test]
fn the_fixture_verifier_authenticates_the_documented_fixture_password() {
    let authenticator = authenticator();

    for artifact in VALID_ARTIFACTS {
        let verifier = restored_verifier(artifact);
        // `Verified` is only reachable through the closed profile allowlist: a
        // verifier outside it is verified against the decoy instead and denied,
        // so this single assertion covers acceptance and verification together.
        // `replacement: None` additionally pins the verifier to the *current*
        // profile rather than to a merely accepted one.
        assert_eq!(
            authenticator.authenticate(
                StoredCredential::Verifier(&verifier),
                FIXTURE_ADMINISTRATOR_PASSWORD.as_bytes(),
            ),
            Ok(PasswordVerdict::Verified { replacement: None }),
            "{artifact}"
        );
    }
}

#[test]
fn the_fixture_verifier_denies_any_other_password() {
    let authenticator = authenticator();

    for artifact in VALID_ARTIFACTS {
        let verifier = restored_verifier(artifact);
        for password in [
            "",
            "fixture-administrator-passwor",
            "fixture-administrator-password ",
            "Fixture-Administrator-Password",
        ] {
            assert_eq!(
                authenticator
                    .authenticate(StoredCredential::Verifier(&verifier), password.as_bytes(),),
                Ok(PasswordVerdict::Denied),
                "{artifact} against {password:?}"
            );
        }
    }
}

#[test]
fn the_fixture_verifier_encodes_exactly_the_current_approved_profile() {
    for artifact in VALID_ARTIFACTS {
        let verifier = restored_verifier(artifact);
        let parsed = PasswordHash::new(&verifier).expect("the fixture verifier parses");

        assert_eq!(
            Algorithm::try_from(parsed.algorithm).expect("a known Argon2 variant"),
            CURRENT_ARGON2_PROFILE.algorithm(),
            "{artifact}"
        );
        assert_eq!(
            Version::try_from(parsed.version.expect("an explicit version"))
                .expect("a known Argon2 version"),
            CURRENT_ARGON2_PROFILE.version(),
            "{artifact}"
        );

        let params = Params::try_from(&parsed).expect("the encoded parameters parse");
        assert_eq!(
            params.m_cost(),
            CURRENT_ARGON2_PROFILE.memory_kib(),
            "{artifact}"
        );
        assert_eq!(
            params.t_cost(),
            CURRENT_ARGON2_PROFILE.iterations(),
            "{artifact}"
        );
        assert_eq!(
            params.p_cost(),
            CURRENT_ARGON2_PROFILE.lanes(),
            "{artifact}"
        );
        assert_eq!(
            params.output_len(),
            Some(CURRENT_ARGON2_PROFILE.output_bytes()),
            "{artifact}"
        );

        let mut decoded = [0_u8; Salt::MAX_LENGTH];
        let salt = parsed
            .salt
            .expect("an encoded salt")
            .decode_b64(&mut decoded)
            .expect("the encoded salt decodes");
        assert_eq!(
            salt.len(),
            CURRENT_ARGON2_PROFILE.salt_bytes(),
            "{artifact}"
        );
        assert_eq!(
            parsed.hash.expect("an encoded output").len(),
            CURRENT_ARGON2_PROFILE.output_bytes(),
            "{artifact}"
        );
    }
}
