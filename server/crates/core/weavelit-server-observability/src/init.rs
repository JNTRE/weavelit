//! Init completion result produced for the System Log.

use weavelit_server_database::{
    CompletionObligation, DeploymentIdentifier, LogClassification, LogDetail, StateIdentifier,
    WorkflowKind,
};
use weavelit_server_log::{CompleteLogRecord, CorrelationId, EventTime, LogResult, SystemLogBody};

use crate::{ObservabilityError, ServerObservability};

/// Classification carried by every Init completion result.
const INIT_CLASSIFICATION: &str = "lifecycle.init";

/// Record and obligation describing one Init completion result.
///
/// Both halves are built together from the same fields so the record delivered
/// after the commit cannot drift from the obligation persisted with the state.
#[derive(Debug)]
pub struct PreparedInitCompletion {
    record: CompleteLogRecord,
    obligation: CompletionObligation,
}

impl PreparedInitCompletion {
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

    /// Returns the obligation committed with the initialized application state.
    #[must_use]
    pub const fn obligation(&self) -> &CompletionObligation {
        &self.obligation
    }
}

impl ServerObservability {
    /// Prepares the Init completion result before application-state commit.
    ///
    /// The detail names only the new deployment, so the record carries no
    /// recovery key, delivery nonce, administrator password, Log Module secret,
    /// or first-administrator identity.
    pub fn prepare_init_completion(
        &self,
        record_identifier: StateIdentifier,
        deployment_identifier: DeploymentIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
    ) -> Result<PreparedInitCompletion, ObservabilityError> {
        let event_time = u64::try_from(event_time_milliseconds)
            .map_err(|_| ObservabilityError::InvalidEventTime)?;
        let detail = init_detail(deployment_identifier);

        let obligation = CompletionObligation::new(
            record_identifier,
            WorkflowKind::Init,
            LogClassification::new(INIT_CLASSIFICATION)
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
            SystemLogBody::new(INIT_CLASSIFICATION, detail)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
        )
        .map_err(|_| ObservabilityError::InvalidLogRecord)?;

        Ok(PreparedInitCompletion { record, obligation })
    }
}

/// Renders the fixed non-secret detail naming the initialized deployment.
fn init_detail(deployment_identifier: DeploymentIdentifier) -> String {
    let mut detail = String::from("initialization completed for deployment ");
    for byte in deployment_identifier.as_bytes() {
        detail.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
        detail.push(char::from_digit(u32::from(byte & 0x0f), 16).unwrap_or('0'));
    }
    detail
}
