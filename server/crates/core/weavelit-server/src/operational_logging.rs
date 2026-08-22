//! Normal-operation support for Audit Log destination failures.
//!
//! This boundary neither decides whether an operation is consequential nor
//! performs one. An owning workflow calls it only after synchronous Audit Log
//! delivery fails; it attempts the corresponding System Log once and returns
//! the one stable rejection that prevents a consequential mutation.

use std::{error::Error as StdError, fmt, sync::Arc};

use weavelit_server_database::StateIdentifier;
use weavelit_server_log::{
    AuditLogClassification, ConfiguredLogDestination, LogDeliveryError, LogModuleIdentifier,
    TrustedRecordIssuer,
};
use weavelit_server_observability::ServerObservability;

use crate::authentication::{random_bytes, system_clock};

const AUDIT_TERMINAL_RECOVERY_CORRELATION_ID: &str = "audit-terminal-recovery";
const AUDIT_TERMINAL_RECOVERY_CLASSIFICATION: AuditLogClassification =
    AuditLogClassification::InternalLogPolicyChanged;

/// Stable client-facing code for an unavailable required Audit Log.
pub const AUDIT_LOG_UNAVAILABLE_CODE: &str = "audit_log_unavailable";

/// A consequential operation rejected because its Audit Log could not be delivered.
///
/// This unit variant retains no destination failure, record, operation, or
/// request content. Transport-specific status and response mapping remain with
/// the future owning route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsequentialOperationError {
    /// Required Audit Log delivery did not complete.
    AuditLogUnavailable,
}

impl ConsequentialOperationError {
    /// Returns the stable error code future Client Modules may map.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::AuditLogUnavailable => AUDIT_LOG_UNAVAILABLE_CODE,
        }
    }
}

impl fmt::Display for ConsequentialOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuditLogUnavailable => {
                formatter.write_str("Audit Log unavailable; operation rejected.")
            }
        }
    }
}

impl StdError for ConsequentialOperationError {}

/// Best-effort System Log reporting for normal-operation Audit failures.
pub struct OperationalLogSupport {
    observability: ServerObservability,
    system_log: Option<Arc<ConfiguredLogDestination>>,
}

impl OperationalLogSupport {
    /// Composes support with the deployment's opened System Log, when available.
    #[must_use]
    pub const fn new(
        record_issuer: TrustedRecordIssuer,
        system_log: Option<Arc<ConfiguredLogDestination>>,
    ) -> Self {
        Self {
            observability: ServerObservability::new(record_issuer),
            system_log,
        }
    }

    /// Reports one Audit delivery failure and rejects a consequential operation.
    ///
    /// The original error is consumed but never inspected or rendered. System
    /// record construction and delivery are each attempted at most once; any
    /// failure is absorbed so this support cannot crash the Server or replace
    /// the stable rejection with an observability failure.
    #[allow(clippy::too_many_arguments)]
    pub fn reject_consequential_audit_failure(
        &self,
        delivery_error: LogDeliveryError,
        record_identifier: StateIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
        destination_module: &LogModuleIdentifier,
        operation: AuditLogClassification,
    ) -> ConsequentialOperationError {
        let _ = delivery_error;
        self.report_audit_failure(
            record_identifier,
            event_time_milliseconds,
            correlation_identifier,
            destination_module,
            operation,
        );

        ConsequentialOperationError::AuditLogUnavailable
    }

    /// Reports one terminal-recovery failure without creating a client mapping.
    pub(crate) fn report_audit_terminal_recovery_failure(
        &self,
        destination_module: &LogModuleIdentifier,
    ) {
        let clock = system_clock();
        let (Some(entropy), Some(event_time_milliseconds)) = (random_bytes::<16>(), clock()) else {
            return;
        };
        let Ok(record_identifier) = StateIdentifier::from_bytes(entropy) else {
            return;
        };
        self.report_audit_failure(
            record_identifier,
            event_time_milliseconds,
            AUDIT_TERMINAL_RECOVERY_CORRELATION_ID,
            destination_module,
            AUDIT_TERMINAL_RECOVERY_CLASSIFICATION,
        );
    }

    fn report_audit_failure(
        &self,
        record_identifier: StateIdentifier,
        event_time_milliseconds: i64,
        correlation_identifier: &str,
        destination_module: &LogModuleIdentifier,
        operation: AuditLogClassification,
    ) {
        if let Some(destination) = self.system_log.as_ref()
            && let Ok(record) = self.observability.prepare_audit_log_unavailable(
                record_identifier,
                event_time_milliseconds,
                correlation_identifier,
                destination_module,
                operation,
            )
        {
            let _ = destination.deliver(&record);
        }
    }
}

impl fmt::Debug for OperationalLogSupport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("OperationalLogSupport(REDACTED)")
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use weavelit_server_database::StateIdentifier;
    use weavelit_server_log::{
        AuditLogClassification, CompleteLogRecord, DurableAcknowledgement, LogCapabilities,
        LogDeliveryError, LogDestination, LogDestinationError, LogDestinationFactory,
        LogModuleCatalog, LogModuleFactoryContext, LogModuleIdentifier, LogModuleRegistration,
        LogRecordPersistenceView, LogRecordType, LogSettingsContract, TrustedLogModuleContext,
        TrustedRecordIssuer,
    };
    use weavelit_server_log_authority::ServerLogAuthority;

    use super::{AUDIT_LOG_UNAVAILABLE_CODE, ConsequentialOperationError, OperationalLogSupport};

    #[derive(Debug, Eq, PartialEq)]
    struct DeliveredSystemRecord {
        classification: String,
        detail: String,
        correlation_identifier: String,
    }

    struct RecordingDestination {
        records: Arc<Mutex<Vec<DeliveredSystemRecord>>>,
        attempts: Arc<AtomicUsize>,
        unavailable: bool,
    }

    impl LogDestination for RecordingDestination {
        fn deliver(
            &self,
            record: &CompleteLogRecord,
            acknowledgement: DurableAcknowledgement,
        ) -> Result<DurableAcknowledgement, LogDestinationError> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self.unavailable {
                return Err(LogDestinationError::Unavailable);
            }
            let LogRecordPersistenceView::System(view) = record.persistence_view() else {
                panic!("the support must deliver only a System record");
            };
            self.records
                .lock()
                .expect("the record lock must not poison")
                .push(DeliveredSystemRecord {
                    classification: view.body().classification().to_owned(),
                    detail: view.body().detail().to_owned(),
                    correlation_identifier: view.correlation_id().as_str().to_owned(),
                });
            Ok(acknowledgement)
        }

        fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
            Ok(())
        }
    }

    struct RecordingFactory {
        records: Arc<Mutex<Vec<DeliveredSystemRecord>>>,
        attempts: Arc<AtomicUsize>,
        unavailable: bool,
    }

    impl LogDestinationFactory for RecordingFactory {
        fn accepted_settings(&self) -> LogSettingsContract {
            LogSettingsContract::none()
        }

        fn create(
            &self,
            _context: &LogModuleFactoryContext<'_>,
        ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
            Ok(Box::new(RecordingDestination {
                records: Arc::clone(&self.records),
                attempts: Arc::clone(&self.attempts),
                unavailable: self.unavailable,
            }))
        }
    }

    fn support(
        system_unavailable: bool,
    ) -> (
        OperationalLogSupport,
        Arc<Mutex<Vec<DeliveredSystemRecord>>>,
        Arc<AtomicUsize>,
    ) {
        let records = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
            "sqlite",
            LogCapabilities::new(vec![LogRecordType::System])
                .expect("the test capability must be valid"),
            Box::new(RecordingFactory {
                records: Arc::clone(&records),
                attempts: Arc::clone(&attempts),
                unavailable: system_unavailable,
            }),
        )])
        .expect("the test catalog must be valid");
        let module = LogModuleIdentifier::new("sqlite").expect("the module identifier is valid");
        let destination = catalog
            .create_destination(
                &module,
                &TrustedLogModuleContext::from_server_authority(
                    &ServerLogAuthority::new(),
                    "/unused".into(),
                    [0x11; 16],
                ),
            )
            .expect("the test destination must open");
        let support = OperationalLogSupport::new(
            TrustedRecordIssuer::from_server_authority(&ServerLogAuthority::new()),
            Some(Arc::new(destination)),
        );
        (support, records, attempts)
    }

    fn identifier(byte: u8) -> StateIdentifier {
        StateIdentifier::from_bytes([byte; 16]).expect("a non-zero identifier is valid")
    }

    fn audit_failure() -> LogDeliveryError {
        LogDeliveryError::Destination(LogDestinationError::Unavailable)
    }

    #[test]
    fn a_consequential_failure_records_safe_context_and_returns_one_stable_error() {
        let (support, records, attempts) = support(false);
        let rejection = support.reject_consequential_audit_failure(
            audit_failure(),
            identifier(0x21),
            1_700_000_000_000,
            "correlation-01",
            &LogModuleIdentifier::new("sqlite").expect("the module identifier is valid"),
            AuditLogClassification::AuthenticationUserCreated,
        );

        assert_eq!(rejection, ConsequentialOperationError::AuditLogUnavailable);
        assert_eq!(rejection.code(), AUDIT_LOG_UNAVAILABLE_CODE);
        assert_eq!(
            rejection.to_string(),
            "Audit Log unavailable; operation rejected."
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        assert_eq!(
            *records.lock().expect("the record lock must not poison"),
            vec![DeliveredSystemRecord {
                classification: "dependency.audit-log-unavailable".to_owned(),
                detail:
                    "audit destination module sqlite unavailable for authentication.user.created"
                        .to_owned(),
                correlation_identifier: "correlation-01".to_owned(),
            }]
        );

        let rendered = format!("{rejection} {rejection:?}");
        for forbidden in [
            "sqlite",
            "database is locked",
            "authentication.user.created",
        ] {
            assert!(!rendered.contains(forbidden), "{rendered}");
        }
    }

    #[test]
    fn an_unavailable_system_log_does_not_change_the_rejection_or_stop_later_calls() {
        let (support, records, attempts) = support(true);
        let module = LogModuleIdentifier::new("sqlite").expect("the module identifier is valid");

        for byte in [0x31, 0x32] {
            assert_eq!(
                support.reject_consequential_audit_failure(
                    audit_failure(),
                    identifier(byte),
                    1_700_000_000_000,
                    "correlation-01",
                    &module,
                    AuditLogClassification::AuthenticationUserCreated,
                ),
                ConsequentialOperationError::AuditLogUnavailable
            );
        }

        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(
            records
                .lock()
                .expect("the record lock must not poison")
                .is_empty()
        );
    }

    #[test]
    fn every_delivery_error_maps_to_the_same_payload_free_rejection() {
        let support = OperationalLogSupport::new(
            TrustedRecordIssuer::from_server_authority(&ServerLogAuthority::new()),
            None,
        );
        let module = LogModuleIdentifier::new("sqlite").expect("the module identifier is valid");
        let failures = [
            LogDeliveryError::CapabilityUnavailable,
            LogDeliveryError::IntegrityFailure,
            LogDeliveryError::Destination(LogDestinationError::ConfigurationInvalid),
            LogDeliveryError::Destination(LogDestinationError::Unavailable),
            LogDeliveryError::Destination(LogDestinationError::IntegrityFailure),
        ];

        for (index, failure) in failures.into_iter().enumerate() {
            assert_eq!(
                support.reject_consequential_audit_failure(
                    failure,
                    identifier(u8::try_from(index + 1).expect("the index fits")),
                    1,
                    "correlation-01",
                    &module,
                    AuditLogClassification::AuthenticationUserCreated,
                ),
                ConsequentialOperationError::AuditLogUnavailable
            );
        }
    }

    #[test]
    fn a_failed_record_construction_still_returns_rejection_with_no_delivery_attempts() {
        let (support, records, attempts) = support(false);
        let module = LogModuleIdentifier::new("sqlite").expect("the module identifier is valid");
        let rejection = support.reject_consequential_audit_failure(
            audit_failure(),
            identifier(0x41),
            i64::MAX,
            "",
            &module,
            AuditLogClassification::AuthenticationUserCreated,
        );

        assert_eq!(rejection, ConsequentialOperationError::AuditLogUnavailable);
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
        assert!(
            records
                .lock()
                .expect("the record lock must not poison")
                .is_empty()
        );
    }
}
