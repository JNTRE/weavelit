use std::{error::Error as StdError, fmt};

/// Invalid submitted recovery key rejected before any decryption.
///
/// Variants exist for validation-order attribution inside the workspace. Their
/// display representation is uniform, so no variant tells a caller which part
/// of a submitted value was wrong.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryKeyError {
    /// The submitted value was empty.
    Empty,
    /// The submitted value exceeded the accepted canonical length.
    TooLong,
    /// The submitted value carried more than one line.
    NotSingleLine,
    /// The submitted value carried surrounding whitespace or other content.
    SurroundingContent,
    /// The submitted value did not use a canonical age Bech32 encoding.
    NotCanonical,
    /// A canonical public recipient was submitted where an identity is required.
    IdentityRequired,
}

impl fmt::Display for RecoveryKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("recovery key is invalid")
    }
}

impl StdError for RecoveryKeyError {}

/// A recovery key, delivery nonce, or proof value could not be produced.
///
/// This is the Server-side preparation failure, not a rejected submission. Its
/// display representation carries no cryptographic, dependency, or
/// operating-system detail, and a caller maps it to its own redacted category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RecoveryKeyPreparationError {
    /// Operating-system randomness was unavailable.
    ///
    /// Randomness has no deterministic fallback here; a failure stops the
    /// operation that needed it rather than producing a predictable key or
    /// nonce.
    RandomnessUnavailable,
    /// A canonical encoding or proof value could not be produced.
    PreparationFailed,
}

impl fmt::Display for RecoveryKeyPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("recovery key preparation failed")
    }
}

impl StdError for RecoveryKeyPreparationError {}
