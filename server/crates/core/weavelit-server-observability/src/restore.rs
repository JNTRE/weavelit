//! Restore completion result produced for the System Log.

use weavelit_server_database::{
    CompletionObligation, DeploymentIdentifier, LogClassification, LogDetail, StateIdentifier,
    WorkflowKind,
};
use weavelit_server_log::{CompleteLogRecord, CorrelationId, EventTime, LogResult, SystemLogBody};

use crate::{ObservabilityError, ServerObservability};

/// Classification carried by every Restore completion result.
const RESTORE_CLASSIFICATION: &str = "lifecycle.restore";

/// Record and obligation describing one Restore completion result.
///
/// Both halves are built together from the same fields so the record delivered
/// after the commit cannot drift from the obligation persisted with the state.
#[derive(Debug)]
pub struct PreparedRestoreCompletion {
    record: CompleteLogRecord,
    obligation: CompletionObligation,
}

impl PreparedRestoreCompletion {
    /// Returns the record to deliver and the obligation to persist.
    #[must_use]
    pub fn into_parts(self) -> (CompleteLogRecord, CompletionObligation) {
        (self.record, self.obligation)
    }

    /// Returns the record to deliver to the assigned System Log destination.
    #[must_use]
    pub const fn record(&self) -> &CompleteLogRecord {
        &self.record
    }

    /// Returns the obligation committed with the restored application state.
    #[must_use]
    pub const fn obligation(&self) -> &CompletionObligation {
        &self.obligation
    }
}

impl ServerObservability {
    /// Prepares the Restore completion result before application-state commit.
    ///
    /// The detail names only the replacement deployment, so the record carries
    /// no recovery key, backup content, or restored identity.
    pub fn prepare_restore_completion(
        &self,
        record_identifier: StateIdentifier,
        deployment_identifier: DeploymentIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
    ) -> Result<PreparedRestoreCompletion, ObservabilityError> {
        let event_time = u64::try_from(event_time_milliseconds)
            .map_err(|_| ObservabilityError::InvalidEventTime)?;
        let detail = restore_detail(deployment_identifier);

        let obligation = CompletionObligation::new(
            record_identifier,
            WorkflowKind::Restore,
            LogClassification::new(RESTORE_CLASSIFICATION)
                .map_err(|_| ObservabilityError::InvalidCompletionObligation)?,
            weavelit_server_database::CorrelationIdentifier::new(correlation_identifier)
                .map_err(|_| ObservabilityError::InvalidCompletionObligation)?,
            event_time_milliseconds,
            LogDetail::new(detail.clone())
                .map_err(|_| ObservabilityError::InvalidCompletionObligation)?,
        )
        .map_err(|_| ObservabilityError::InvalidCompletionObligation)?;

        let record = CompleteLogRecord::system(
            self.record_issuer
                .issue(*record_identifier.as_bytes())
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            EventTime::from_unix_milliseconds(event_time),
            LogResult::Success,
            CorrelationId::new(correlation_identifier)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            SystemLogBody::new(RESTORE_CLASSIFICATION, detail)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
        )
        .map_err(|_| ObservabilityError::InvalidLogRecord)?;

        Ok(PreparedRestoreCompletion { record, obligation })
    }
}

/// Renders the fixed non-secret detail naming the replacement deployment.
fn restore_detail(deployment_identifier: DeploymentIdentifier) -> String {
    let mut detail = String::from("restore completed for deployment ");
    for byte in deployment_identifier.as_bytes() {
        detail.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        detail.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    detail
}
