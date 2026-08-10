//! Server Observability: construction of pre-redacted System Log records.
//!
//! Canonical authority assigns System record construction and pre-redaction to
//! Server Observability, including the Init and Restore completion results. A
//! workflow crate therefore never builds a record itself; it asks Observability
//! for a prepared completion and receives both the record to deliver and the
//! matching obligation to persist.
//!
//! Milestone 1 needs only the Restore completion result. The crate is
//! nonetheless the long-term owner of Server-produced operational telemetry, so
//! each event family lives in its own module.

#![forbid(unsafe_code)]

mod restore;

pub use restore::PreparedRestoreCompletion;

use weavelit_server_log::TrustedRecordIssuer;

/// Stable failure to construct a Server-owned observability record.
///
/// Variants carry no event content, so a rendered error cannot disclose the
/// values that failed validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObservabilityError {
    /// The supplied event time is outside the representable range.
    InvalidEventTime,
    /// The completion obligation rejected a supplied field.
    InvalidCompletionObligation,
    /// The complete log record rejected a supplied field.
    InvalidLogRecord,
}

impl core::fmt::Display for ObservabilityError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidEventTime => "observability event time is invalid",
            Self::InvalidCompletionObligation => "observability completion obligation is invalid",
            Self::InvalidLogRecord => "observability log record is invalid",
        })
    }
}

impl core::error::Error for ObservabilityError {}

/// Server-owned producer of complete, pre-redacted System Log records.
pub struct ServerObservability {
    record_issuer: TrustedRecordIssuer,
}

impl ServerObservability {
    /// Creates the producer from the Server-owned record issuer.
    #[must_use]
    pub const fn new(record_issuer: TrustedRecordIssuer) -> Self {
        Self { record_issuer }
    }
}
