#![forbid(unsafe_code)]

//! The canonical age recovery key shared by every Weavelit Server workflow.
//!
//! This crate owns one accepted recovery-key spelling and one representation of
//! it: canonical age Bech32 parsing and encoding, the X25519 key agreement a
//! backup reader needs, and the redacted, clearing secret types that carry a
//! private identity. Init additionally uses it to generate a key pair, mint a
//! unique delivery nonce, compute the expected HMAC-SHA-256 proof of
//! possession, and compare a submitted proof in constant time.
//!
//! Init and Restore both depend on this crate and not on each other. Moving the
//! representation here does not change the accepted key syntax: a submitted
//! backup key is parsed by exactly the code that parsed it before.
//!
//! The private key is never persisted in any form, including as an HMAC key.
//! Only the public recipient, the delivery nonce, and the expected proof value
//! are ever recorded.

mod delivery;
mod error;
mod key;
mod proof;

pub use delivery::PreparedRecoveryKey;
pub use error::{RecoveryKeyError, RecoveryKeyPreparationError};
pub use key::{
    IDENTITY_PREFIX, KEY_LENGTH, MAX_RECOVERY_KEY_LENGTH, RECIPIENT_PREFIX, RecoveryIdentity,
    RecoveryKey, RecoveryRecipient,
};
pub use proof::{DELIVERY_NONCE_BYTES, DeliveryNonce, RECOVERY_PROOF_BYTES, RecoveryProof};
