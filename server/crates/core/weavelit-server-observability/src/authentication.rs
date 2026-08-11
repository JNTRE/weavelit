//! Local authentication failure result produced for the System Log.

use weavelit_server_database::StateIdentifier;
use weavelit_server_log::{CompleteLogRecord, CorrelationId, EventTime, LogResult, SystemLogBody};

use crate::{ObservabilityError, ServerObservability};

/// Classification carried by every local authentication failure result.
///
/// This is the registered System Log taxonomy value for an authentication
/// failure. A destination tolerates an additively registered value, so an
/// unregistered classification would still be stored, which is precisely why
/// this must be pinned to the taxonomy rather than invented here.
const AUTHENTICATION_FAILURE_CLASSIFICATION: &str = "authentication.failure";

/// Detail carried by every local authentication failure result.
///
/// It is a compile-time constant with no interpolation, so no username,
/// account identifier, password, token, address, or other client-supplied text
/// can reach the record through it. An unknown account, an inactive account, an
/// account without a usable verifier, and a wrong password all produce this one
/// detail, so the System Log cannot separate them either.
const AUTHENTICATION_FAILURE_DETAIL: &str = "local password authentication denied";

impl ServerObservability {
    /// Prepares the System Log result for one denied local authentication.
    ///
    /// The record carries a fresh Server-generated identifier, the event time,
    /// the correlation identifier the response already reports, and the fixed
    /// classification and detail above. It carries nothing derived from the
    /// request.
    pub fn prepare_authentication_failure(
        &self,
        record_identifier: StateIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
    ) -> Result<CompleteLogRecord, ObservabilityError> {
        let event_time = u64::try_from(event_time_milliseconds)
            .map_err(|_| ObservabilityError::InvalidEventTime)?;

        CompleteLogRecord::system(
            self.record_issuer
                .issue(*record_identifier.as_bytes())
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            EventTime::from_unix_milliseconds(event_time),
            LogResult::Failure,
            CorrelationId::new(correlation_identifier)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            SystemLogBody::new(
                AUTHENTICATION_FAILURE_CLASSIFICATION,
                AUTHENTICATION_FAILURE_DETAIL,
            )
            .map_err(|_| ObservabilityError::InvalidLogRecord)?,
        )
        .map_err(|_| ObservabilityError::InvalidLogRecord)
    }
}

#[cfg(test)]
mod tests {
    use weavelit_server_database::StateIdentifier;
    use weavelit_server_log::{LogRecordPersistenceView, LogResult, TrustedRecordIssuer};
    use weavelit_server_log_authority::ServerLogAuthority;

    use super::{AUTHENTICATION_FAILURE_CLASSIFICATION, AUTHENTICATION_FAILURE_DETAIL};
    use crate::{ObservabilityError, ServerObservability};

    /// Every value a denied attempt could plausibly leak.
    const IDENTIFYING_TEXT: [&str; 6] = [
        "admin",
        "operator",
        "correct horse battery staple",
        "0123456789abcdef",
        "127.0.0.1",
        "web-ui",
    ];

    fn observability() -> ServerObservability {
        ServerObservability::new(TrustedRecordIssuer::from_server_authority(
            &ServerLogAuthority::new(),
        ))
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).expect("a non-zero identifier is valid")
    }

    #[test]
    fn a_failure_record_carries_only_its_fixed_classification_and_detail() {
        let record = observability()
            .prepare_authentication_failure(identifier(0x21), 1_700_000_000_000, "correlation-01")
            .expect("the fixed record must construct");

        let LogRecordPersistenceView::System(view) = record.persistence_view() else {
            panic!("an authentication failure must be a System record");
        };
        assert_eq!(view.result(), LogResult::Failure);
        assert_eq!(view.correlation_id().as_str(), "correlation-01");
        // Pinned to the literal registered taxonomy value rather than to the
        // constant, because comparing the constant to itself would accept any
        // classification this crate happened to declare.
        assert_eq!(view.body().classification(), "authentication.failure");
        assert_eq!(
            view.body().classification(),
            AUTHENTICATION_FAILURE_CLASSIFICATION
        );
        assert_eq!(view.body().detail(), AUTHENTICATION_FAILURE_DETAIL);
        assert_eq!(view.event_time().unix_milliseconds(), 1_700_000_000_000);
        assert_eq!(view.record_id().as_bytes(), &[0x21; 16]);

        for text in IDENTIFYING_TEXT {
            assert!(!view.body().classification().contains(text), "{text}");
            assert!(!view.body().detail().contains(text), "{text}");
        }
    }

    #[test]
    fn every_denial_produces_the_same_body_whatever_its_cause() {
        let observability = observability();
        let first = observability
            .prepare_authentication_failure(identifier(0x31), 1, "correlation-01")
            .expect("the fixed record must construct");
        let second = observability
            .prepare_authentication_failure(identifier(0x32), 2, "correlation-02")
            .expect("the fixed record must construct");

        let (LogRecordPersistenceView::System(first), LogRecordPersistenceView::System(second)) =
            (first.persistence_view(), second.persistence_view())
        else {
            panic!("an authentication failure must be a System record");
        };
        assert_eq!(first.body(), second.body());
    }

    #[test]
    fn an_unrepresentable_event_time_or_correlation_is_refused() {
        let observability = observability();
        assert_eq!(
            observability
                .prepare_authentication_failure(identifier(0x41), -1, "correlation-01")
                .unwrap_err(),
            ObservabilityError::InvalidEventTime
        );
        assert_eq!(
            observability
                .prepare_authentication_failure(identifier(0x42), 1, "")
                .unwrap_err(),
            ObservabilityError::InvalidLogRecord
        );
    }
}
