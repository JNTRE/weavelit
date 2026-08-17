//! Init recovery-key delivery nonce and proof of possession.
//!
//! Init delivers the private recovery key exactly once and then requires the
//! requesting client to prove it retained the key. The Server records only the
//! public recipient, the delivery nonce, and the expected proof value; the
//! private key is never persisted, including as an HMAC key.

use std::fmt;

use hmac::{Hmac, KeyInit as _, Mac as _};
use sha2::Sha256;
use subtle::ConstantTimeEq as _;

use crate::{RecoveryIdentity, RecoveryKeyPreparationError};

/// Bytes carried by one recovery-key delivery nonce.
///
/// Thirty-two bytes is 256 bits, which makes a repeated nonce across
/// deployments practically impossible without coordinating state.
pub const DELIVERY_NONCE_BYTES: usize = 32;

/// Bytes carried by one recovery-key proof value.
///
/// HMAC-SHA-256 produces exactly this many bytes, and the proof is never
/// truncated.
pub const RECOVERY_PROOF_BYTES: usize = 32;

/// Unique delivery nonce recorded in the pending Init checkpoint.
///
/// The nonce is not a secret, but it is never rendered: a checkpoint value that
/// appears in a log or an error invites correlating a deployment's retained
/// state with its delivery.
#[derive(Clone, Copy)]
pub struct DeliveryNonce([u8; DELIVERY_NONCE_BYTES]);

impl DeliveryNonce {
    /// Generates a unique delivery nonce from operating-system randomness.
    pub fn generate() -> Result<Self, RecoveryKeyPreparationError> {
        let mut bytes = [0_u8; DELIVERY_NONCE_BYTES];
        getrandom::fill(&mut bytes)
            .map_err(|_| RecoveryKeyPreparationError::RandomnessUnavailable)?;
        Ok(Self(bytes))
    }

    /// Reconstructs a nonce read back from a stored Init checkpoint.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; DELIVERY_NONCE_BYTES]) -> Self {
        Self(bytes)
    }

    /// Returns the nonce bytes recorded in the Init checkpoint.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; DELIVERY_NONCE_BYTES] {
        &self.0
    }
}

impl fmt::Debug for DeliveryNonce {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DeliveryNonce(REDACTED)")
    }
}

/// One expected or submitted recovery-key proof of possession.
///
/// The type deliberately implements neither `PartialEq` nor `Display`, so the
/// only available comparison is the constant-time [`RecoveryProof::matches`]
/// and no code path can render it.
#[derive(Clone, Copy)]
pub struct RecoveryProof([u8; RECOVERY_PROOF_BYTES]);

impl RecoveryProof {
    /// Computes HMAC-SHA-256 over the delivery nonce keyed by the private key.
    ///
    /// The message is exactly the delivery nonce and the key is exactly the
    /// delivered private key's raw bytes, so the client that retained the key
    /// can reproduce this value from the nonce alone. The identity's raw bytes
    /// key the computation and never leave it: the Server computes this value
    /// while it still holds the delivered key and records only the result.
    pub fn compute(
        identity: &RecoveryIdentity,
        nonce: &DeliveryNonce,
    ) -> Result<Self, RecoveryKeyPreparationError> {
        let key = identity.secret_bytes();
        let mut mac = Hmac::<Sha256>::new_from_slice(&key[..])
            .map_err(|_| RecoveryKeyPreparationError::PreparationFailed)?;
        mac.update(nonce.as_bytes());
        Ok(Self(mac.finalize().into_bytes().into()))
    }

    /// Reconstructs a proof read back from a stored Init checkpoint or a
    /// submitted request.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; RECOVERY_PROOF_BYTES]) -> Self {
        Self(bytes)
    }

    /// Returns the proof bytes recorded in the Init checkpoint.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; RECOVERY_PROOF_BYTES] {
        &self.0
    }

    /// Compares a submitted proof against this stored expected proof without a
    /// data-dependent branch or early return.
    #[must_use]
    pub fn matches(&self, submitted: &Self) -> bool {
        self.0.ct_eq(&submitted.0).into()
    }
}

impl fmt::Debug for RecoveryProof {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryProof(REDACTED)")
    }
}
