//! Opaque process-memory credential for one TOTP enablement preview.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(crate) const TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES: usize = 32;
pub(crate) const TOTP_ENABLEMENT_PREVIEW_TEXT_BYTES: usize = 43;

const _: () = assert!(
    TOTP_ENABLEMENT_PREVIEW_TEXT_BYTES == TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES.div_ceil(3) * 4 - 1,
    "the TOTP enablement preview length must match its approved entropy"
);

const DIGEST_DOMAIN: &[u8] = b"weavelit.administration.totp-enablement-preview.v1";

pub(crate) struct TotpEnablementPreviewCredential {
    text: Zeroizing<String>,
}

impl TotpEnablementPreviewCredential {
    pub(crate) fn generate() -> Option<Self> {
        let mut entropy = Zeroizing::new([0_u8; TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES]);
        getrandom::fill(&mut *entropy).ok()?;
        Some(Self::from_zeroizing_entropy(entropy))
    }

    fn from_zeroizing_entropy(
        entropy: Zeroizing<[u8; TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES]>,
    ) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.text
    }

    pub(crate) fn digest(&self) -> TotpEnablementPreviewDigest {
        TotpEnablementPreviewDigest::of_canonical(&self.text)
            .expect("a generated TOTP enablement preview is canonical")
    }
}

impl fmt::Debug for TotpEnablementPreviewCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TotpEnablementPreviewCredential(REDACTED)")
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TotpEnablementPreviewDigest([u8; 32]);

impl TotpEnablementPreviewDigest {
    pub(crate) fn of_canonical(preview: &str) -> Option<Self> {
        if preview.len() != TOTP_ENABLEMENT_PREVIEW_TEXT_BYTES {
            return None;
        }
        let decoded = URL_SAFE_NO_PAD.decode(preview).ok()?;
        if decoded.len() != TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES
            || URL_SAFE_NO_PAD.encode(&decoded) != preview
        {
            return None;
        }
        let mut digest = Sha256::new();
        digest.update(DIGEST_DOMAIN);
        digest.update(preview.as_bytes());
        Some(Self(digest.finalize().into()))
    }

    pub(crate) fn matches(&self, other: &Self) -> bool {
        self.0
            .iter()
            .zip(other.0.iter())
            .fold(0_u8, |difference, (stored, submitted)| {
                difference | (stored ^ submitted)
            })
            == 0
    }
}

impl fmt::Debug for TotpEnablementPreviewDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TotpEnablementPreviewDigest(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(seed: u8) -> TotpEnablementPreviewCredential {
        let mut entropy = [0_u8; TOTP_ENABLEMENT_PREVIEW_ENTROPY_BYTES];
        entropy[0] = seed;
        TotpEnablementPreviewCredential::from_zeroizing_entropy(Zeroizing::new(entropy))
    }

    #[test]
    fn preview_is_canonical_domain_separated_and_redacted() {
        let preview = seeded(1);
        let other = seeded(2);
        let plain: [u8; 32] = Sha256::digest(preview.as_str().as_bytes()).into();

        assert_eq!(preview.as_str().len(), TOTP_ENABLEMENT_PREVIEW_TEXT_BYTES);
        assert!(preview.digest().matches(&preview.digest()));
        assert!(!preview.digest().matches(&other.digest()));
        assert!(
            !preview
                .digest()
                .matches(&TotpEnablementPreviewDigest(plain))
        );
        assert_eq!(
            format!("{preview:?}"),
            "TotpEnablementPreviewCredential(REDACTED)"
        );
        assert_eq!(
            format!("{:?}", preview.digest()),
            "TotpEnablementPreviewDigest(REDACTED)"
        );
    }

    #[test]
    fn generated_previews_differ_and_noncanonical_values_are_rejected() {
        let first = TotpEnablementPreviewCredential::generate().unwrap();
        let second = TotpEnablementPreviewCredential::generate().unwrap();
        assert_ne!(first.as_str(), second.as_str());
        assert!(TotpEnablementPreviewDigest::of_canonical("short").is_none());
        assert!(
            TotpEnablementPreviewDigest::of_canonical(&format!("{}=", first.as_str())).is_none()
        );
    }
}
