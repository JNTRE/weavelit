//! Audit Log destination unavailability produced for the System Log.

use weavelit_server_database::StateIdentifier;
use weavelit_server_log::{
    AuditLogClassification, CompleteLogRecord, CorrelationId, EventTime, LogModuleIdentifier,
    LogResult, SystemLogBody, SystemLogClassification,
};

use crate::{ObservabilityError, ServerObservability};

const AUDIT_LOG_UNAVAILABLE_CLASSIFICATION: SystemLogClassification =
    SystemLogClassification::DependencyAuditLogUnavailable;

impl ServerObservability {
    /// Prepares the System Log result for an Audit Log delivery failure.
    ///
    /// The destination and operation are already validated, closed-domain
    /// values. No destination error or Audit record content enters this
    /// boundary, so the resulting detail cannot disclose either one.
    pub fn prepare_audit_log_unavailable(
        &self,
        record_identifier: StateIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
        destination_module: &LogModuleIdentifier,
        operation: AuditLogClassification,
    ) -> Result<CompleteLogRecord, ObservabilityError> {
        let event_time = u64::try_from(event_time_milliseconds)
            .map_err(|_| ObservabilityError::InvalidEventTime)?;
        let detail = format!(
            "audit destination module {} unavailable for {}",
            destination_module.as_str(),
            operation.as_str()
        );

        CompleteLogRecord::system(
            self.record_issuer
                .issue(*record_identifier.as_bytes())
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            EventTime::from_unix_milliseconds(event_time),
            LogResult::Failure,
            CorrelationId::new(correlation_identifier)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
            SystemLogBody::new(AUDIT_LOG_UNAVAILABLE_CLASSIFICATION, detail)
                .map_err(|_| ObservabilityError::InvalidLogRecord)?,
        )
        .map_err(|_| ObservabilityError::InvalidLogRecord)
    }
}

#[cfg(test)]
mod tests {
    use weavelit_server_database::StateIdentifier;
    use weavelit_server_log::{
        AuditLogClassification, LogModuleIdentifier, LogRecordPersistenceView, LogResult,
        TrustedRecordIssuer,
    };
    use weavelit_server_log_authority::ServerLogAuthority;

    use crate::{ObservabilityError, ServerObservability};

    fn observability() -> ServerObservability {
        ServerObservability::new(TrustedRecordIssuer::from_server_authority(
            &ServerLogAuthority::new(),
        ))
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).expect("a non-zero identifier is valid")
    }

    fn destination() -> LogModuleIdentifier {
        LogModuleIdentifier::new("sqlite").expect("the built-in module identifier is valid")
    }

    #[test]
    fn an_audit_delivery_failure_carries_only_typed_safe_context() {
        let record = observability()
            .prepare_audit_log_unavailable(
                identifier(0x21),
                1_700_000_000_000,
                "correlation-01",
                &destination(),
                AuditLogClassification::AuthenticationUserCreated,
            )
            .expect("the typed failure record must construct");

        let LogRecordPersistenceView::System(view) = record.persistence_view() else {
            panic!("an Audit destination failure must be a System record");
        };
        assert_eq!(view.result(), LogResult::Failure);
        assert_eq!(view.correlation_id().as_str(), "correlation-01");
        assert_eq!(
            view.body().classification(),
            "dependency.audit-log-unavailable"
        );
        assert_eq!(
            view.body().detail(),
            "audit destination module sqlite unavailable for authentication.user.created"
        );
        assert_eq!(view.event_time().unix_milliseconds(), 1_700_000_000_000);
        assert_eq!(view.record_id().as_bytes(), &[0x21; 16]);

        for forbidden in [
            "database is locked",
            "permission denied",
            "request payload",
            "provider credential",
        ] {
            assert!(!view.body().detail().contains(forbidden), "{forbidden}");
        }
    }

    #[test]
    fn invalid_event_metadata_is_refused_without_rendering_it() {
        let invalid_time = observability()
            .prepare_audit_log_unavailable(
                identifier(0x31),
                -1,
                "correlation-01",
                &destination(),
                AuditLogClassification::AuthenticationUserCreated,
            )
            .unwrap_err();
        let invalid_correlation = observability()
            .prepare_audit_log_unavailable(
                identifier(0x32),
                1,
                "",
                &destination(),
                AuditLogClassification::AuthenticationUserCreated,
            )
            .unwrap_err();

        assert_eq!(invalid_time, ObservabilityError::InvalidEventTime);
        assert_eq!(invalid_correlation, ObservabilityError::InvalidLogRecord);
        for error in [invalid_time, invalid_correlation] {
            let rendered = format!("{error} {error:?}");
            assert!(!rendered.contains("correlation-01"), "{rendered}");
            assert!(
                !rendered.contains("authentication.user.created"),
                "{rendered}"
            );
        }
    }
}
