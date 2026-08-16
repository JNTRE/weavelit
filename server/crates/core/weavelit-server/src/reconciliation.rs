//! Opaque lifecycle reconciliation capabilities held only by the submitting browser.

use std::fmt;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use weavelit_server_database::ReconciliationDigest;
use zeroize::Zeroizing;

/// Entropy in one opaque lifecycle reconciliation capability.
pub const RECONCILIATION_CAPABILITY_ENTROPY_BYTES: usize = 32;

const RECONCILIATION_CAPABILITY_DOMAIN: &[u8] = b"weavelit.lifecycle.reconciliation.v1";

/// A browser-held bearer capability that proves which lifecycle submission
/// completed. The raw value is never persisted and clears when dropped.
pub struct ReconciliationCapability {
    text: Zeroizing<String>,
}

impl ReconciliationCapability {
    /// Mints a capability from protected operating-system entropy supplied by the caller.
    #[must_use]
    pub fn from_zeroizing_entropy(
        entropy: Zeroizing<[u8; RECONCILIATION_CAPABILITY_ENTROPY_BYTES]>,
    ) -> Self {
        Self {
            text: Zeroizing::new(URL_SAFE_NO_PAD.encode(&entropy[..])),
        }
    }

    /// Takes ownership of one capability submitted through the Client Module.
    ///
    /// The shared Client Module already validated its bounded opaque shape and
    /// owns the request buffer that delivered it. This type keeps that value in
    /// a clearing owner until its domain-separated digest is computed.
    pub(crate) fn from_submitted(text: Zeroizing<String>) -> Self {
        Self { text }
    }

    /// Returns the value for the single response that delivers it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Returns the domain-separated digest that may be retained durably.
    #[must_use]
    pub fn digest(&self) -> ReconciliationDigest {
        let mut digest = Sha256::new();
        digest.update(RECONCILIATION_CAPABILITY_DOMAIN);
        digest.update(self.text.as_bytes());
        ReconciliationDigest::from_bytes(digest.finalize().into())
    }
}

impl fmt::Debug for ReconciliationCapability {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReconciliationCapability(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use super::{RECONCILIATION_CAPABILITY_ENTROPY_BYTES, ReconciliationCapability};
    use zeroize::Zeroizing;

    #[test]
    fn a_submitted_capability_has_the_digest_of_the_value_that_was_issued() {
        let issued = ReconciliationCapability::from_zeroizing_entropy(Zeroizing::new(
            [0xA5; RECONCILIATION_CAPABILITY_ENTROPY_BYTES],
        ));
        let submitted =
            ReconciliationCapability::from_submitted(Zeroizing::new(issued.as_str().to_owned()));

        assert!(issued.digest().matches(&submitted.digest()));
    }
}
