//! Authorization denial result produced for the System Log.

use weavelit_server_database::StateIdentifier;
use weavelit_server_log::{
    CompleteLogRecord, CorrelationId, EventTime, LogResult, SystemLogBody, SystemLogClassification,
};

use crate::{ObservabilityError, ServerObservability};

/// Classification carried by every authorization denial result.
///
/// This is the registered System Log taxonomy value for an authorization denial.
const AUTHORIZATION_DENIAL_CLASSIFICATION: SystemLogClassification =
    SystemLogClassification::AuthorizationDenial;

/// Detail carried by every authorization denial result.
///
/// It is a compile-time constant with no interpolation, so no account,
/// username, session, Client Module, plane, Service Module, Operation, Group,
/// grant, enablement state, Service Connection, request value, or internal
/// reason can reach the record through it. An inactive account, a disabled or
/// uncatalogued component, and every missing grant all produce this one detail,
/// so the System Log cannot separate them either.
const AUTHORIZATION_DENIAL_DETAIL: &str = "request authorization denied";

impl ServerObservability {
    /// Prepares the System Log result for one denied authorization.
    ///
    /// The record carries a fresh Server-generated identifier, the event time,
    /// the correlation identifier the response already reports, and the fixed
    /// classification and detail above. Those three values are the only fields
    /// that vary between denials; it carries nothing derived from the request,
    /// the account, or the decision.
    pub fn prepare_authorization_denial(
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
                AUTHORIZATION_DENIAL_CLASSIFICATION,
                AUTHORIZATION_DENIAL_DETAIL,
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

    use super::{AUTHORIZATION_DENIAL_CLASSIFICATION, AUTHORIZATION_DENIAL_DETAIL};
    use crate::{ObservabilityError, ServerObservability};

    /// Every value a denied request could plausibly leak.
    const IDENTIFYING_TEXT: [&str; 14] = [
        "admin",
        "operator",
        "0123456789abcdef",
        "127.0.0.1",
        "web-ui",
        "user",
        "administration",
        "zendesk",
        "ticket",
        "group",
        "grant",
        "disabled",
        "inactive",
        "connection",
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
    fn a_denial_record_carries_only_its_fixed_classification_and_detail() {
        let record = observability()
            .prepare_authorization_denial(identifier(0x51), 1_700_000_000_000, "correlation-01")
            .expect("the fixed record must construct");

        let LogRecordPersistenceView::System(view) = record.persistence_view() else {
            panic!("an authorization denial must be a System record");
        };
        assert_eq!(view.result(), LogResult::Failure);
        assert_eq!(view.correlation_id().as_str(), "correlation-01");
        // Pinned to the literal registered taxonomy value rather than to the
        // constant, because comparing the constant to itself would accept any
        // classification this crate happened to declare.
        assert_eq!(view.body().classification(), "authorization.denial");
        assert_eq!(
            view.body().classification(),
            AUTHORIZATION_DENIAL_CLASSIFICATION.as_str()
        );
        // Pinned literally for the same reason.
        assert_eq!(view.body().detail(), "request authorization denied");
        assert_eq!(view.body().detail(), AUTHORIZATION_DENIAL_DETAIL);
        assert_eq!(view.event_time().unix_milliseconds(), 1_700_000_000_000);
        assert_eq!(view.record_id().as_bytes(), &[0x51; 16]);

        let scanned = format!(
            "{} {}",
            view.body().classification().to_ascii_lowercase(),
            view.body().detail().to_ascii_lowercase()
        );
        for text in IDENTIFYING_TEXT {
            assert!(!scanned.contains(text), "{text}");
        }
    }

    #[test]
    fn every_denial_produces_the_same_body_whatever_its_cause() {
        let observability = observability();
        let first = observability
            .prepare_authorization_denial(identifier(0x61), 1, "correlation-01")
            .expect("the fixed record must construct");
        let second = observability
            .prepare_authorization_denial(identifier(0x62), 2, "correlation-02")
            .expect("the fixed record must construct");

        let (LogRecordPersistenceView::System(first), LogRecordPersistenceView::System(second)) =
            (first.persistence_view(), second.persistence_view())
        else {
            panic!("an authorization denial must be a System record");
        };
        // Only the record identifier, the event time, and the correlation
        // identifier may differ between two denials.
        assert_eq!(first.body(), second.body());
        assert_eq!(first.result(), second.result());
        assert_ne!(first.record_id().as_bytes(), second.record_id().as_bytes());
        assert_ne!(
            first.event_time().unix_milliseconds(),
            second.event_time().unix_milliseconds()
        );
        assert_ne!(
            first.correlation_id().as_str(),
            second.correlation_id().as_str()
        );
    }

    #[test]
    fn a_denial_record_is_never_confused_with_an_authentication_failure() {
        let observability = observability();
        let denial = observability
            .prepare_authorization_denial(identifier(0x71), 1, "correlation-01")
            .expect("the fixed record must construct");
        let failure = observability
            .prepare_authentication_failure(identifier(0x71), 1, "correlation-01")
            .expect("the fixed record must construct");

        let (LogRecordPersistenceView::System(denial), LogRecordPersistenceView::System(failure)) =
            (denial.persistence_view(), failure.persistence_view())
        else {
            panic!("both records must be System records");
        };
        assert_ne!(denial.body(), failure.body());
    }

    #[test]
    fn an_unrepresentable_event_time_or_correlation_is_refused() {
        let observability = observability();
        assert_eq!(
            observability
                .prepare_authorization_denial(identifier(0x81), -1, "correlation-01")
                .unwrap_err(),
            ObservabilityError::InvalidEventTime
        );
        assert_eq!(
            observability
                .prepare_authorization_denial(identifier(0x82), 1, "")
                .unwrap_err(),
            ObservabilityError::InvalidLogRecord
        );
    }
}
