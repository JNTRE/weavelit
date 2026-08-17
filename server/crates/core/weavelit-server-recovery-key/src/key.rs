//! Canonical age recovery-key parsing, generation, and encoding.
//!
//! The accepted syntax is exactly one canonical age Bech32 line: a lowercase
//! `age1...` public recipient or an uppercase `AGE-SECRET-KEY-1...` private
//! identity. Parsing, encoding, and key agreement all live here so Init and
//! Restore share one representation and one accepted spelling.

use std::fmt;

use bech32::{Bech32, Hrp, primitives::decode::CheckedHrpstring};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::{Zeroize as _, Zeroizing};

use crate::{RecoveryKeyError, RecoveryKeyPreparationError};

/// Canonical age Bech32 human-readable prefix of a private recovery identity.
pub const IDENTITY_PREFIX: &str = "AGE-SECRET-KEY-1";

/// Canonical age Bech32 human-readable prefix of a public recovery recipient.
pub const RECIPIENT_PREFIX: &str = "age1";

/// Maximum accepted canonical recovery-key line length in bytes.
pub const MAX_RECOVERY_KEY_LENGTH: usize = 128;

/// Byte length of an X25519 scalar or public key.
pub const KEY_LENGTH: usize = 32;

/// Bech32 human-readable part of a private recovery identity.
const IDENTITY_HRP: &str = "AGE-SECRET-KEY-";

/// Bech32 human-readable part of a public recovery recipient.
const RECIPIENT_HRP: &str = "age";

/// Private X25519 recovery identity held only in bounded transient memory.
///
/// The wrapped secret is cleared on drop and never enters a display, debug,
/// log, or client-visible representation. The type is deliberately not `Clone`,
/// so a delivered private key cannot be duplicated into a longer-lived value.
pub struct RecoveryIdentity(StaticSecret);

impl RecoveryIdentity {
    /// Generates a new recovery identity from operating-system randomness.
    pub fn generate() -> Result<Self, RecoveryKeyPreparationError> {
        let mut secret = [0_u8; KEY_LENGTH];
        getrandom::fill(&mut secret)
            .map_err(|_| RecoveryKeyPreparationError::RandomnessUnavailable)?;
        let identity = Self(StaticSecret::from(secret));
        secret.zeroize();
        Ok(identity)
    }

    /// Returns the recipient public key this identity corresponds to.
    pub fn public_key(&self) -> [u8; KEY_LENGTH] {
        PublicKey::from(&self.0).to_bytes()
    }

    /// Returns the public recovery recipient this identity corresponds to.
    pub fn recipient(&self) -> RecoveryRecipient {
        RecoveryRecipient(self.public_key())
    }

    /// Encodes the canonical uppercase `AGE-SECRET-KEY-1...` delivery line.
    ///
    /// The returned line is cleared on drop. It exists only long enough to be
    /// written to the one response that delivers it.
    pub fn encode(&self) -> Result<Zeroizing<String>, RecoveryKeyPreparationError> {
        let secret = self.secret_bytes();
        encode_canonical(IDENTITY_HRP, &secret, Case::Upper).map(Zeroizing::new)
    }

    /// Performs the age X25519 key agreement against a header ephemeral share.
    ///
    /// Returns `None` when the agreed secret is non-contributory, which is an
    /// ephemeral share of small order that contributed nothing to the
    /// agreement; the age specification requires rejecting it.
    pub fn agree(&self, share: &[u8; KEY_LENGTH]) -> Option<Zeroizing<[u8; KEY_LENGTH]>> {
        let shared = self.0.diffie_hellman(&PublicKey::from(*share));
        shared
            .was_contributory()
            .then(|| Zeroizing::new(shared.to_bytes()))
    }

    /// Returns the raw private scalar this identity encodes.
    ///
    /// Deliberately crate-private: the raw bytes are needed to key the delivery
    /// proof and to produce the canonical delivery line, and nothing outside
    /// this crate may obtain them.
    pub(crate) fn secret_bytes(&self) -> Zeroizing<[u8; KEY_LENGTH]> {
        Zeroizing::new(self.0.to_bytes())
    }
}

impl fmt::Debug for RecoveryIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryIdentity(REDACTED)")
    }
}

/// Public X25519 recovery recipient.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryRecipient([u8; KEY_LENGTH]);

impl RecoveryRecipient {
    /// Returns the recipient public key bytes.
    pub const fn public_key(&self) -> [u8; KEY_LENGTH] {
        self.0
    }

    /// Encodes the canonical lowercase `age1...` recipient line.
    ///
    /// The recipient is not secret: it is the only recovery-key value a Server
    /// retains.
    pub fn encode(&self) -> Result<String, RecoveryKeyPreparationError> {
        encode_canonical(RECIPIENT_HRP, &self.0, Case::Lower)
    }
}

/// One canonical age recovery key accepted from a client.
pub enum RecoveryKey {
    /// An uppercase `AGE-SECRET-KEY-1...` private identity.
    Identity(RecoveryIdentity),
    /// A lowercase `age1...` public recipient.
    Recipient(RecoveryRecipient),
}

impl RecoveryKey {
    /// Parses exactly one canonical age Bech32 line.
    ///
    /// A single trailing newline terminates the line and is accepted. Any other
    /// surrounding content, additional line, mixed case, failed checksum, wrong
    /// payload length, or non-canonical encoding is rejected before any
    /// decryption is attempted.
    pub fn parse(submitted: &str) -> Result<Self, RecoveryKeyError> {
        let line = canonical_line(submitted)?;

        if line.starts_with(IDENTITY_PREFIX) {
            if line.chars().any(|character| character.is_ascii_lowercase()) {
                return Err(RecoveryKeyError::NotCanonical);
            }
            let secret = decode_canonical(line, IDENTITY_HRP, Case::Upper)?;
            return Ok(Self::Identity(RecoveryIdentity(StaticSecret::from(secret))));
        }

        if line.starts_with(RECIPIENT_PREFIX) {
            if line.chars().any(|character| character.is_ascii_uppercase()) {
                return Err(RecoveryKeyError::NotCanonical);
            }
            let public = decode_canonical(line, RECIPIENT_HRP, Case::Lower)?;
            return Ok(Self::Recipient(RecoveryRecipient(public)));
        }

        Err(RecoveryKeyError::NotCanonical)
    }

    /// Returns the private identity required to decrypt a submitted backup.
    pub fn into_identity(self) -> Result<RecoveryIdentity, RecoveryKeyError> {
        match self {
            Self::Identity(identity) => Ok(identity),
            Self::Recipient(_) => Err(RecoveryKeyError::IdentityRequired),
        }
    }
}

impl fmt::Debug for RecoveryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RecoveryKey(REDACTED)")
    }
}

/// Canonical letter case of one age Bech32 encoding.
#[derive(Clone, Copy)]
enum Case {
    Lower,
    Upper,
}

/// Encodes one 32-byte payload as a canonical age Bech32 line.
///
/// This is the single encoder the decoder's canonicity check is defined
/// against, so a generated line is spelled exactly as a submitted line must be.
fn encode_canonical(
    expected_hrp: &str,
    payload: &[u8; KEY_LENGTH],
    case: Case,
) -> Result<String, RecoveryKeyPreparationError> {
    let hrp =
        Hrp::parse(expected_hrp).map_err(|_| RecoveryKeyPreparationError::PreparationFailed)?;
    match case {
        Case::Lower => bech32::encode_lower::<Bech32>(hrp, payload),
        Case::Upper => bech32::encode_upper::<Bech32>(hrp, payload),
    }
    .map_err(|_| RecoveryKeyPreparationError::PreparationFailed)
}

/// Decodes one canonical age Bech32 line into its exact 32-byte payload.
///
/// The Bech32 checksum, the expected human-readable part, the exact payload
/// length, and the canonical encoding are all enforced: re-encoding the decoded
/// payload must reproduce the submitted line byte for byte, so a Bech32m
/// checksum, padded payload, or any other non-canonical spelling is rejected.
fn decode_canonical(
    line: &str,
    expected_hrp: &str,
    case: Case,
) -> Result<[u8; KEY_LENGTH], RecoveryKeyError> {
    let hrp = Hrp::parse(expected_hrp).map_err(|_| RecoveryKeyError::NotCanonical)?;
    let checked =
        CheckedHrpstring::new::<Bech32>(line).map_err(|_| RecoveryKeyError::NotCanonical)?;
    if checked.hrp() != hrp {
        return Err(RecoveryKeyError::NotCanonical);
    }

    let payload: [u8; KEY_LENGTH] = checked
        .byte_iter()
        .collect::<Vec<u8>>()
        .try_into()
        .map_err(|_| RecoveryKeyError::NotCanonical)?;

    let canonical = match case {
        Case::Lower => bech32::encode_lower::<Bech32>(hrp, &payload),
        Case::Upper => bech32::encode_upper::<Bech32>(hrp, &payload),
    }
    .map_err(|_| RecoveryKeyError::NotCanonical)?;
    if canonical != line {
        return Err(RecoveryKeyError::NotCanonical);
    }

    Ok(payload)
}

fn canonical_line(submitted: &str) -> Result<&str, RecoveryKeyError> {
    if submitted.is_empty() {
        return Err(RecoveryKeyError::Empty);
    }
    if submitted.len() > MAX_RECOVERY_KEY_LENGTH {
        return Err(RecoveryKeyError::TooLong);
    }

    let line = submitted.strip_suffix('\n').unwrap_or(submitted);
    if line.is_empty() {
        return Err(RecoveryKeyError::Empty);
    }
    if line.contains('\n') || line.contains('\r') {
        return Err(RecoveryKeyError::NotSingleLine);
    }
    if line.chars().any(|character| {
        character.is_whitespace() || character.is_control() || !character.is_ascii()
    }) {
        return Err(RecoveryKeyError::SurroundingContent);
    }

    Ok(line)
}
