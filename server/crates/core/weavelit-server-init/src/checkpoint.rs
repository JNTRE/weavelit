//! One-time recovery-key delivery and the checkpoint it is confirmed against.
//!
//! Init delivers the private recovery key exactly once and then requires the
//! requesting client to prove it retained the key. Only the public recipient,
//! the delivery nonce, and the expected proof value are recordable; the private
//! key is never persisted in any form, including as an HMAC key.

use std::fmt;

use weavelit_server_database::{
    CheckpointMetadata, MAX_RECOVERY_PUBLIC_KEY_LENGTH, RecoveryPublicKey,
};
use weavelit_server_recovery_key::{
    DELIVERY_NONCE_BYTES, DeliveryNonce, PreparedRecoveryKey, RECOVERY_PROOF_BYTES, RecoveryProof,
};
use zeroize::Zeroizing;

use crate::{CheckpointError, InitError};

/// Version of the Init checkpoint metadata encoding.
///
/// The version is the first byte of every encoding, so a future layout is
/// distinguishable from this one rather than being reinterpreted as it.
pub const CHECKPOINT_FORMAT_VERSION: u8 = 1;

/// Fixed bytes an encoding carries before the variable-length recipient.
const FIXED_PREFIX_BYTES: usize = 1 + DELIVERY_NONCE_BYTES + RECOVERY_PROOF_BYTES + 1;

/// The Init checkpoint's recorded recovery-key delivery.
///
/// Its constructor and fields are private to this crate, so the only ways to
/// obtain one are preparing a delivery and decoding metadata this Server wrote.
/// A caller therefore cannot present a checkpoint whose expected proof it chose,
/// which is what makes the constant-time comparison in [`InitCheckpoint::confirm`]
/// meaningful.
#[derive(Clone)]
pub struct InitCheckpoint {
    recovery_public_key: RecoveryPublicKey,
    delivery_nonce: DeliveryNonce,
    expected_proof: RecoveryProof,
}

impl InitCheckpoint {
    pub(crate) const fn new(
        recovery_public_key: RecoveryPublicKey,
        delivery_nonce: DeliveryNonce,
        expected_proof: RecoveryProof,
    ) -> Self {
        Self {
            recovery_public_key,
            delivery_nonce,
            expected_proof,
        }
    }

    /// Returns the public recipient retained for future backups.
    #[must_use]
    pub const fn recovery_public_key(&self) -> &RecoveryPublicKey {
        &self.recovery_public_key
    }

    /// Returns the delivery nonce the proof is computed over.
    #[must_use]
    pub const fn delivery_nonce(&self) -> &DeliveryNonce {
        &self.delivery_nonce
    }

    /// Compares a submitted proof against the stored expected proof.
    ///
    /// The comparison is constant time and has no data-dependent branch. An
    /// absent proof is its own category, so a client that never submitted one
    /// is told what to do rather than being told its proof was wrong.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::RecoveryKeyConfirmationRequired`] when no proof was
    /// submitted, and [`InitError::RecoveryKeyConfirmationInvalid`] when the
    /// submitted proof does not match.
    pub fn confirm(&self, submitted: Option<&RecoveryProof>) -> Result<(), InitError> {
        let submitted = submitted.ok_or(InitError::RecoveryKeyConfirmationRequired)?;
        if self.expected_proof.matches(submitted) {
            Ok(())
        } else {
            Err(InitError::RecoveryKeyConfirmationInvalid)
        }
    }

    /// Encodes the fixed versioned layout recorded in the Init checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::InitializationFailed`] when the encoding exceeds
    /// what the persistence contract accepts.
    pub fn encode(&self) -> Result<CheckpointMetadata, InitError> {
        let recipient = self.recovery_public_key.as_str().as_bytes();
        let length = u8::try_from(recipient.len()).map_err(|_| InitError::InitializationFailed)?;

        let mut encoded = Vec::with_capacity(FIXED_PREFIX_BYTES + recipient.len());
        encoded.push(CHECKPOINT_FORMAT_VERSION);
        encoded.extend_from_slice(self.delivery_nonce.as_bytes());
        encoded.extend_from_slice(self.expected_proof.as_bytes());
        encoded.push(length);
        encoded.extend_from_slice(recipient);

        CheckpointMetadata::from_bytes(encoded).map_err(|_| InitError::InitializationFailed)
    }

    /// Decodes checkpoint metadata this Server previously wrote.
    ///
    /// Every field is length-checked and the recipient is revalidated as a
    /// canonical recovery public key, so retained metadata that no longer has
    /// the fixed layout fails closed instead of yielding a partially trusted
    /// checkpoint.
    ///
    /// # Errors
    ///
    /// Returns [`CheckpointError`] when the version, the fixed layout, or the
    /// encoded recipient is not what this Server writes.
    pub fn decode(metadata: &CheckpointMetadata) -> Result<Self, CheckpointError> {
        let bytes = metadata.as_bytes();
        let (version, rest) = bytes.split_first().ok_or(CheckpointError::Malformed)?;
        if *version != CHECKPOINT_FORMAT_VERSION {
            return Err(CheckpointError::UnsupportedFormatVersion);
        }

        let (nonce, rest) = split_array::<DELIVERY_NONCE_BYTES>(rest)?;
        let (proof, rest) = split_array::<RECOVERY_PROOF_BYTES>(rest)?;
        let (length, recipient) = rest.split_first().ok_or(CheckpointError::Malformed)?;
        if usize::from(*length) != recipient.len()
            || recipient.len() > MAX_RECOVERY_PUBLIC_KEY_LENGTH
        {
            return Err(CheckpointError::Malformed);
        }

        let recipient = std::str::from_utf8(recipient).map_err(|_| CheckpointError::Malformed)?;
        let recovery_public_key =
            RecoveryPublicKey::new(recipient).map_err(|_| CheckpointError::RecipientInvalid)?;

        Ok(Self {
            recovery_public_key,
            delivery_nonce: DeliveryNonce::from_bytes(nonce),
            expected_proof: RecoveryProof::from_bytes(proof),
        })
    }
}

impl fmt::Debug for InitCheckpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("InitCheckpoint(REDACTED)")
    }
}

/// A freshly prepared recovery key and the checkpoint that will confirm it.
///
/// The private key leaves this value exactly once, as the canonical delivery
/// line returned by [`PreparedInitDelivery::into_delivery_line`], and is
/// cleared as this value is consumed. Nothing here exposes it otherwise.
pub struct PreparedInitDelivery {
    prepared: PreparedRecoveryKey,
    checkpoint: InitCheckpoint,
}

impl PreparedInitDelivery {
    /// Generates the key pair and delivery nonce, then computes the expected proof.
    ///
    /// The shared recovery-key crate performs the required order, so the
    /// checkpoint this returns cannot exist before its expected proof does.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::InitializationFailed`] when randomness is
    /// unavailable or a canonical encoding or proof cannot be produced.
    pub fn prepare() -> Result<Self, InitError> {
        let prepared = PreparedRecoveryKey::prepare()?;
        let recipient = prepared.recipient().encode()?;
        let recovery_public_key =
            RecoveryPublicKey::new(recipient).map_err(|_| InitError::InitializationFailed)?;
        let checkpoint = InitCheckpoint::new(
            recovery_public_key,
            *prepared.delivery_nonce(),
            *prepared.expected_proof(),
        );

        Ok(Self {
            prepared,
            checkpoint,
        })
    }

    /// Returns the checkpoint the Init checkpoint records.
    #[must_use]
    pub const fn checkpoint(&self) -> &InitCheckpoint {
        &self.checkpoint
    }

    /// Consumes the preparation and returns the canonical delivery line.
    ///
    /// # Errors
    ///
    /// Returns [`InitError::InitializationFailed`] when the canonical encoding
    /// cannot be produced.
    pub fn into_delivery_line(self) -> Result<Zeroizing<String>, InitError> {
        Ok(self.prepared.into_delivery_line()?)
    }
}

impl fmt::Debug for PreparedInitDelivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PreparedInitDelivery(REDACTED)")
    }
}

fn split_array<const BYTES: usize>(input: &[u8]) -> Result<([u8; BYTES], &[u8]), CheckpointError> {
    let (head, rest) = input
        .split_at_checked(BYTES)
        .ok_or(CheckpointError::Malformed)?;
    let head = <[u8; BYTES]>::try_from(head).map_err(|_| CheckpointError::Malformed)?;
    Ok((head, rest))
}
