use std::fmt;

use bech32::{Bech32, Hrp, primitives::decode::CheckedHrpstring};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

use crate::RecoveryKeyError;

/// Canonical age Bech32 human-readable prefix of a private recovery identity.
pub const IDENTITY_PREFIX: &str = "AGE-SECRET-KEY-1";

/// Canonical age Bech32 human-readable prefix of a public recovery recipient.
pub const RECIPIENT_PREFIX: &str = "age1";

/// Maximum accepted canonical recovery-key line length in bytes.
pub const MAX_RECOVERY_KEY_LENGTH: usize = 128;

/// Byte length of an X25519 scalar or public key.
pub(crate) const KEY_LENGTH: usize = 32;

/// Bech32 human-readable part of a private recovery identity.
const IDENTITY_HRP: &str = "AGE-SECRET-KEY-";

/// Bech32 human-readable part of a public recovery recipient.
const RECIPIENT_HRP: &str = "age";

/// Private X25519 recovery identity held only in bounded transient memory.
///
/// The wrapped secret is cleared on drop and never enters a display, debug,
/// log, or client-visible representation.
pub struct RecoveryIdentity(StaticSecret);

impl RecoveryIdentity {
    /// Returns the recipient public key this identity corresponds to.
    pub fn public_key(&self) -> [u8; KEY_LENGTH] {
        PublicKey::from(&self.0).to_bytes()
    }

    /// Performs the age X25519 key agreement against a header ephemeral share.
    pub(crate) fn diffie_hellman(&self, share: &PublicKey) -> SharedSecret {
        self.0.diffie_hellman(share)
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
