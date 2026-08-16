//! The TOTP secret and the verification decision made against it.

use std::fmt;

use totp_rs::{Builder, Secret, Totp};
use zeroize::Zeroizing;

use crate::provisioning::{ProvisioningError, ProvisioningText, provisioning_uri};
use crate::{DIGITS, SECRET_LENGTH, SKEW_STEPS, STEP_SECONDS};

/// One RFC 6238 time step a code was matched in.
///
/// The type is ordered because ordering is the only thing the replay watermark
/// asks of it: a code is accepted only when its step is strictly greater than
/// the last step that factor accepted.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TimeStep(u64);

impl TimeStep {
    /// Returns the step number counted from `T0 = 0`.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0
    }
}

/// A 160-bit TOTP secret, held only in zeroizing and redacting form.
///
/// The type implements neither [`fmt::Display`] nor `PartialEq`, and its
/// [`fmt::Debug`] renders no bytes, so the secret cannot be rendered into a
/// log, an error, or a response body. It is disclosed only through
/// [`TotpSecret::base32`] and [`TotpSecret::provisioning_uri`], which return
/// values with the same properties.
pub struct TotpSecret(Zeroizing<[u8; SECRET_LENGTH]>);

impl TotpSecret {
    /// Adopts an already protected fixed-length secret without another copy.
    ///
    /// Server-owned callers use this when the bytes already live in zeroizing
    /// storage, such as fresh operating-system entropy or decrypted factor
    /// data. The module never generates the bytes itself; randomness has one
    /// owner and it is not this crate.
    #[must_use]
    pub fn from_zeroizing(bytes: Zeroizing<[u8; SECRET_LENGTH]>) -> Self {
        Self(bytes)
    }

    /// Adopts the secret bytes the Server drew from the operating system.
    ///
    /// The parameter is a fixed-length array, so a secret of any other size,
    /// including one shortened by a caller, is not a value this type can be
    /// built from. The module never generates the bytes itself; randomness has
    /// one owner and it is not this crate.
    #[must_use]
    pub fn from_bytes(bytes: [u8; SECRET_LENGTH]) -> Self {
        Self::from_zeroizing(Zeroizing::new(bytes))
    }

    /// Returns the secret's unpadded RFC 4648 Base32 encoding.
    #[must_use]
    pub fn base32(&self) -> ProvisioningText {
        ProvisioningText::new(Secret::new_stack(*self.0).to_base32())
    }

    /// Builds the provisioning URI disclosed once at enrollment.
    ///
    /// `maximum_bytes` is the caller's own bound on the URI it can disclose.
    /// The account label is fitted to it, so a caller that passes the bound its
    /// response profile enforces always receives a URI that profile accepts.
    pub fn provisioning_uri(
        &self,
        issuer: &str,
        account: &str,
        maximum_bytes: usize,
    ) -> Result<ProvisioningText, ProvisioningError> {
        provisioning_uri(issuer, account, &self.base32(), maximum_bytes)
    }

    /// Returns the time step `code` matched at `unix_seconds`, when it matches.
    ///
    /// The instant is supplied rather than read, so the decision is
    /// reproducible. A match alone does not accept a login: the returned step
    /// must still advance the factor's replay watermark, which this crate does
    /// not own.
    #[must_use]
    pub fn verify(&self, code: &str, unix_seconds: u64) -> Option<TimeStep> {
        self.engine().check(code, unix_seconds).map(TimeStep)
    }

    /// Returns the code this secret produces at `unix_seconds`.
    ///
    /// A Weavelit deployment never needs to produce a code: only the user's
    /// authenticator does that, and the Server only ever verifies. Generation
    /// is therefore not part of the shipped surface, and is compiled only when
    /// the `test-support` feature is enabled so that a caller's tests can
    /// exercise an enrollment confirmation end to end. Enabling the feature in
    /// a production dependency edge would be a reviewable manifest change.
    ///
    /// Without this, testing a successful enrollment would mean either
    /// brute-forcing a million candidates through [`Self::verify`] against the
    /// Server's random secret, or leaving the confirmation's success path
    /// uncovered.
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn code_at(&self, unix_seconds: u64) -> String {
        format!("{}", self.engine().generate(unix_seconds))
    }

    /// Builds the profile-bound engine used for one operation.
    ///
    /// Every parameter comes from this module's profile constants, so the
    /// only inputs a caller controls are the secret bytes and the presented
    /// code. The build is fallible only for a value the constants forbid.
    fn engine(&self) -> Totp {
        Builder::new()
            .with_secret(Secret::new_stack(*self.0))
            .with_digits(DIGITS)
            .with_step_duration(STEP_SECONDS)
            .with_skew(SKEW_STEPS)
            .build()
            .expect("the compiled-in TOTP profile is valid")
    }
}

impl fmt::Debug for TotpSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TotpSecret(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The RFC 6238 test secret, whose published vectors this module pins.
    const RFC_6238_SECRET: [u8; SECRET_LENGTH] = *b"12345678901234567890";

    /// The unpadded RFC 4648 Base32 encoding of [`RFC_6238_SECRET`].
    const RFC_6238_BASE32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    /// Published RFC 6238 HMAC-SHA-1 six-digit vectors, as `(instant, code)`.
    const RFC_6238_VECTORS: [(u64, &str); 4] = [
        (59, "287082"),
        (1_111_111_109, "081804"),
        (1_234_567_890, "005924"),
        (2_000_000_000, "279037"),
    ];

    fn secret() -> TotpSecret {
        TotpSecret::from_bytes(RFC_6238_SECRET)
    }

    #[test]
    fn the_secret_encodes_to_the_pinned_unpadded_base32_value() {
        let encoded = secret().base32();

        assert_eq!(encoded.expose(), RFC_6238_BASE32);
        assert_eq!(encoded.expose().len(), 32);
        assert!(!encoded.expose().contains('='));
    }

    #[test]
    fn adoption_of_a_zeroizing_rfc_secret_preserves_its_known_base32_and_code() {
        let secret = TotpSecret::from_zeroizing(Zeroizing::new(RFC_6238_SECRET));

        assert_eq!(secret.base32().expose(), RFC_6238_BASE32);
        assert_eq!(secret.verify("287082", 59), Some(TimeStep(1)));
    }

    #[test]
    fn the_pinned_rfc_6238_vectors_verify_in_their_own_step() {
        let secret = secret();

        for (instant, code) in RFC_6238_VECTORS {
            let step = secret
                .verify(code, instant)
                .unwrap_or_else(|| panic!("the RFC 6238 vector at {instant} must verify"));

            assert_eq!(step, TimeStep(instant / STEP_SECONDS));
        }
    }

    #[test]
    fn a_code_from_another_vector_is_refused_at_this_instant() {
        let secret = secret();

        for (instant, _) in RFC_6238_VECTORS {
            for (other_instant, other_code) in RFC_6238_VECTORS {
                if other_instant == instant {
                    continue;
                }

                assert_eq!(secret.verify(other_code, instant), None);
            }
        }
    }

    #[test]
    fn verification_accepts_exactly_one_step_on_either_side() {
        let secret = secret();
        let (instant, code) = RFC_6238_VECTORS[2];
        let origin = instant / STEP_SECONDS;

        for accepted in [instant - STEP_SECONDS, instant, instant + STEP_SECONDS] {
            assert_eq!(secret.verify(code, accepted), Some(TimeStep(origin)));
        }

        for refused in [instant - 2 * STEP_SECONDS, instant + 2 * STEP_SECONDS] {
            assert_eq!(secret.verify(code, refused), None);
        }
    }

    #[test]
    fn a_code_holds_for_the_whole_step_it_was_generated_in() {
        let secret = secret();
        let (instant, code) = RFC_6238_VECTORS[2];

        assert_eq!(
            secret.verify(code, instant + STEP_SECONDS - 1),
            Some(TimeStep(instant / STEP_SECONDS))
        );
    }

    #[test]
    fn a_malformed_code_is_refused_rather_than_matched() {
        let secret = secret();
        let (instant, code) = RFC_6238_VECTORS[0];

        for presented in ["", "28708", "2870822", "28708a", "+28708", "287083"] {
            assert_eq!(secret.verify(presented, instant), None);
        }

        assert_eq!(secret.verify(code, instant), Some(TimeStep(1)));
    }

    #[test]
    fn a_different_secret_does_not_verify_the_vector() {
        let mut bytes = RFC_6238_SECRET;
        bytes[0] ^= 0x01;
        let (instant, code) = RFC_6238_VECTORS[1];

        assert_eq!(TotpSecret::from_bytes(bytes).verify(code, instant), None);
    }

    #[test]
    fn the_secret_never_renders_its_value() {
        let secret = secret();

        assert_eq!(format!("{secret:?}"), "TotpSecret(REDACTED)");
        assert!(!format!("{secret:?}").contains(RFC_6238_BASE32));
    }

    #[test]
    fn the_provisioning_uri_carries_the_pinned_secret_encoding() {
        let uri = secret()
            .provisioning_uri("Weavelit", "ops+admin@example.com", 288)
            .unwrap();

        assert_eq!(
            uri.expose(),
            "otpauth://totp/Weavelit:ops%2Badmin%40example.com\
             ?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Weavelit\
             &algorithm=SHA1&digits=6&period=30"
        );
    }
}
