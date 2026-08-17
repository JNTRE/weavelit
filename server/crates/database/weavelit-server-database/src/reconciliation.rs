//! Submission-bound lifecycle reconciliation held outside restorable state.
//!
//! A reconciliation capability is a browser-held bearer value. This contract
//! represents only its digest, so a database can retain proof that a specific
//! Init or Restore completed without retaining, rendering, or backing up the
//! capability itself.

use std::fmt;

use subtle::ConstantTimeEq as _;

use crate::DatabaseError;

/// Bytes in a stored lifecycle reconciliation capability digest.
pub const RECONCILIATION_DIGEST_LENGTH: usize = 32;

/// The stored digest of one lifecycle reconciliation capability.
///
/// The type implements neither `PartialEq` nor `Display`, so callers can only
/// compare a submitted digest through the constant-time [`Self::matches`]
/// operation and cannot render the stored value accidentally.
#[derive(Clone, Copy)]
pub struct ReconciliationDigest([u8; RECONCILIATION_DIGEST_LENGTH]);

impl ReconciliationDigest {
    /// Creates a digest from its fixed-size binary representation.
    pub const fn from_bytes(bytes: [u8; RECONCILIATION_DIGEST_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Returns the binary representation for persistence only.
    pub const fn as_bytes(&self) -> &[u8; RECONCILIATION_DIGEST_LENGTH] {
        &self.0
    }

    /// Compares two capability digests without a data-dependent branch.
    pub fn matches(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

impl fmt::Debug for ReconciliationDigest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReconciliationDigest(REDACTED)")
    }
}

/// Live lifecycle reconciliation operations available during normal operation.
///
/// Reconciliation provenance is deliberately not application state: it is
/// neither included in a backup nor restored. A successful Restore replaces
/// the one retained digest atomically with the new submission's digest.
pub trait ReconciliationStore {
    /// Reports whether the supplied digest belongs to the submission that
    /// completed the currently operational deployment.
    fn matches_reconciliation(
        &mut self,
        digest: &ReconciliationDigest,
    ) -> Result<bool, DatabaseError>;
}
