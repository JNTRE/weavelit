use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use crate::RestoreError;

/// Approved maximum encrypted backup artifact size in bytes.
pub const MAX_ENCRYPTED_ARTIFACT_BYTES: usize = 256 * 1024 * 1024;

/// Approved maximum authenticated backup plaintext size in bytes.
pub const MAX_AUTHENTICATED_PLAINTEXT_BYTES: usize = 256 * 1024 * 1024;

/// Approved upload deadline for the encrypted artifact.
pub const UPLOAD_DEADLINE: Duration = Duration::from_secs(120);

/// Approved total deadline for one Restore request.
pub const TOTAL_REQUEST_DEADLINE: Duration = Duration::from_secs(300);

/// Approved number of Restore operations permitted to run at one time.
pub const MAX_CONCURRENT_RESTORE_OPERATIONS: usize = 1;

/// Transfer bounds applied before expensive allocation or state mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TransferBounds {
    /// Maximum accepted encrypted artifact size in bytes.
    pub max_encrypted_artifact_bytes: usize,
    /// Maximum accepted authenticated plaintext size in bytes.
    pub max_authenticated_plaintext_bytes: usize,
}

impl TransferBounds {
    /// The Security Model's approved Restore transfer bounds.
    pub const APPROVED: Self = Self {
        max_encrypted_artifact_bytes: MAX_ENCRYPTED_ARTIFACT_BYTES,
        max_authenticated_plaintext_bytes: MAX_AUTHENTICATED_PLAINTEXT_BYTES,
    };

    /// Rejects an artifact larger than the configured bound.
    pub const fn check_artifact(&self, length: usize) -> Result<(), RestoreError> {
        if length > self.max_encrypted_artifact_bytes {
            return Err(RestoreError::BackupInvalid);
        }
        Ok(())
    }
}

impl Default for TransferBounds {
    fn default() -> Self {
        Self::APPROVED
    }
}

/// Rejects an elapsed upload duration that exceeded the approved deadline.
pub const fn check_upload_elapsed(elapsed: Duration) -> Result<(), RestoreError> {
    if elapsed.as_nanos() > UPLOAD_DEADLINE.as_nanos() {
        return Err(RestoreError::RestoreFailed);
    }
    Ok(())
}

/// Rejects an elapsed request duration that exceeded the approved deadline.
pub const fn check_total_elapsed(elapsed: Duration) -> Result<(), RestoreError> {
    if elapsed.as_nanos() > TOTAL_REQUEST_DEADLINE.as_nanos() {
        return Err(RestoreError::RestoreFailed);
    }
    Ok(())
}

/// The total-deadline check one Restore request is bounded by.
///
/// Validation observes its budget only through this, so a test decides the
/// step an overrun is discovered at instead of waiting for real time to pass.
pub trait RequestDeadline: Send + Sync {
    /// Rejects the request once the approved total deadline has passed.
    fn check(&self) -> Result<(), RestoreError>;
}

/// Monotonic budget for one Restore request's total execution time.
#[derive(Clone, Copy, Debug)]
pub struct RequestBudget {
    started: Instant,
}

impl RequestBudget {
    /// Starts the budget at the current monotonic instant.
    pub fn start() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    /// Creates a budget whose origin is already past the approved deadline.
    ///
    /// Test-only seam: an overrun is otherwise reachable only by waiting out
    /// [`TOTAL_REQUEST_DEADLINE`] in real time. Only an already-exhausted
    /// budget can be built here and the origin is not a caller's to choose, so
    /// nothing can extend, restart, or lengthen a request through it.
    #[doc(hidden)]
    #[must_use]
    pub fn already_exhausted() -> Self {
        Self {
            started: Instant::now()
                .checked_sub(TOTAL_REQUEST_DEADLINE + Duration::from_secs(1))
                .expect("the monotonic clock represents an origin one deadline in the past"),
        }
    }

    /// Returns the elapsed request time.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Returns the time left before the approved total deadline, if any.
    ///
    /// A budget is inherited across the two Restore submission requests, so an
    /// artifact upload is bounded by what remains of the deadline the recovery
    /// key submission started, never by a fresh one.
    pub fn remaining(&self) -> Option<Duration> {
        TOTAL_REQUEST_DEADLINE.checked_sub(self.elapsed())
    }

    /// Rejects the request once the approved total deadline has passed.
    pub fn check(&self) -> Result<(), RestoreError> {
        check_total_elapsed(self.elapsed())
    }
}

impl RequestDeadline for RequestBudget {
    fn check(&self) -> Result<(), RestoreError> {
        check_total_elapsed(self.elapsed())
    }
}

impl Default for RequestBudget {
    fn default() -> Self {
        Self::start()
    }
}

/// In-process gate that admits at most one Restore operation at a time.
#[derive(Debug, Default)]
pub struct RestoreConcurrency {
    occupied: AtomicBool,
}

impl RestoreConcurrency {
    /// Creates an unoccupied gate.
    pub const fn new() -> Self {
        Self {
            occupied: AtomicBool::new(false),
        }
    }

    /// Takes the single Restore slot, or reports a concurrent request.
    pub fn try_acquire(&self) -> Result<RestoreSlot<'_>, RestoreError> {
        self.occupied
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| RestoreSlot { gate: self })
            .map_err(|_| RestoreError::RestorePending)
    }
}

/// Held Restore slot that releases the concurrency gate when dropped.
#[derive(Debug)]
pub struct RestoreSlot<'gate> {
    gate: &'gate RestoreConcurrency,
}

impl Drop for RestoreSlot<'_> {
    fn drop(&mut self) {
        self.gate.occupied.store(false, Ordering::Release);
    }
}
