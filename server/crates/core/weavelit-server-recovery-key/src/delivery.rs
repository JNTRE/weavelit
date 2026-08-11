//! One-time recovery-key preparation for delivery and finalization.

use std::fmt;

use zeroize::Zeroizing;

use crate::{
    DeliveryNonce, RecoveryIdentity, RecoveryKeyPreparationError, RecoveryProof, RecoveryRecipient,
};

/// A freshly generated recovery key prepared for one-time delivery.
///
/// Constructing this value performs the Init design's required order: the key
/// pair is generated, then the unique delivery nonce, then the expected proof.
/// A caller cannot obtain the recipient, nonce, or expected proof it records in
/// the Init checkpoint before all three exist, so the checkpoint write cannot
/// precede proof computation.
///
/// Only [`PreparedRecoveryKey::recipient`], [`PreparedRecoveryKey::delivery_nonce`],
/// and [`PreparedRecoveryKey::expected_proof`] are ever recorded. The private
/// key leaves this value exactly once, as the canonical delivery line returned
/// by [`PreparedRecoveryKey::into_delivery_line`], and is cleared as this value
/// is consumed.
pub struct PreparedRecoveryKey {
    identity: RecoveryIdentity,
    delivery_nonce: DeliveryNonce,
    expected_proof: RecoveryProof,
}

impl PreparedRecoveryKey {
    /// Generates a recovery key pair and delivery nonce, then computes the
    /// expected proof of possession.
    pub fn prepare() -> Result<Self, RecoveryKeyPreparationError> {
        let identity = RecoveryIdentity::generate()?;
        let delivery_nonce = DeliveryNonce::generate()?;
        let expected_proof = RecoveryProof::compute(&identity, &delivery_nonce)?;
        Ok(Self {
            identity,
            delivery_nonce,
            expected_proof,
        })
    }

    /// Returns the public recipient recorded in the Init checkpoint.
    #[must_use]
    pub fn recipient(&self) -> RecoveryRecipient {
        self.identity.recipient()
    }

    /// Returns the delivery nonce recorded in the Init checkpoint.
    #[must_use]
    pub const fn delivery_nonce(&self) -> &DeliveryNonce {
        &self.delivery_nonce
    }

    /// Returns the expected proof recorded in the Init checkpoint.
    #[must_use]
    pub const fn expected_proof(&self) -> &RecoveryProof {
        &self.expected_proof
    }

    /// Consumes the preparation and returns the canonical delivery line.
    ///
    /// The private key is cleared when the returned line is dropped and when
    /// this value is consumed, so no copy of it outlives the response that
    /// carries it.
    pub fn into_delivery_line(self) -> Result<Zeroizing<String>, RecoveryKeyPreparationError> {
        self.identity.encode()
    }
}

impl fmt::Debug for PreparedRecoveryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedRecoveryKey(REDACTED)")
    }
}
