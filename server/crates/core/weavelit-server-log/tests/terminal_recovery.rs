use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use weavelit_server_log::{
    AuditDestinationBinding, AuditDestinationBindingTransition,
    AuditDestinationBindingTransitionError, AuditLogBody, AuditLogClassification, AuditPrincipal,
    AuditPrincipalType, AuditTerminalCompleteness, AuditTerminalRecoveryError,
    AuditTerminalRecoveryProjection, AuditTerminalReplayError,
    AuditTerminalSupersessionAuthorization, AuditTerminalSupersessionConfirmation,
    AuditTerminalSupersessionDisposition, AuditTerminalSupersessionError,
    AuditTerminalSupersessionReason, CompleteLogRecord, ConfiguredLogDestination, CorrelationId,
    DurableAcknowledgement, EventTime, LogCapabilities, LogDestination, LogDestinationError,
    LogDestinationFactory, LogModuleCatalog, LogModuleFactoryContext, LogModuleIdentifier,
    LogModuleRegistration, LogRecordPersistenceView, LogRecordType, LogResult, LogSettingsContract,
    MAX_AUDIT_TERMINAL_RECOVERY_BYTES, MAX_AUDIT_TERMINAL_SUPERSESSION_BYTES,
    ResolvedAuditDestination, SupersededAuditTerminalState, TrustedLogModuleContext,
    TrustedRecordIssuer,
};
use weavelit_server_log_authority::ServerLogAuthority;

fn authority_and_issuer() -> (ServerLogAuthority, TrustedRecordIssuer) {
    let authority = ServerLogAuthority::new();
    let issuer = TrustedRecordIssuer::from_server_authority(&authority);
    (authority, issuer)
}

fn terminal_record(issuer: &TrustedRecordIssuer) -> CompleteLogRecord {
    let correlation = CorrelationId::new("recovery-correlation").unwrap();
    let attempt = CompleteLogRecord::audit_attempt(
        issuer.issue([1; 16]).unwrap(),
        EventTime::from_unix_milliseconds(10),
        correlation,
        AuditLogBody::new(
            AuditLogClassification::AuthenticationUserDisabled,
            AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
            "disable",
            "account:ar-22222222222222222222222222222222",
            "accountable action accepted",
        )
        .unwrap(),
    )
    .unwrap();
    CompleteLogRecord::audit_completion(
        issuer.issue([2; 16]).unwrap(),
        EventTime::from_unix_milliseconds(11),
        attempt.attempt_record_id().unwrap(),
        LogResult::Success,
        CorrelationId::new("recovery-correlation").unwrap(),
        AuditLogBody::new(
            AuditLogClassification::AuthenticationUserDisabled,
            AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
            "disable",
            "account:ar-22222222222222222222222222222222",
            "accountable action completed successfully; account status: disabled",
        )
        .unwrap(),
    )
    .unwrap()
}

fn binding(authority: &ServerLogAuthority, identity: u8, version: u64) -> AuditDestinationBinding {
    AuditDestinationBinding::from_server_authority(authority, [identity; 16], version).unwrap()
}

fn destination(
    authority: &ServerLogAuthority,
    deliveries: Arc<AtomicUsize>,
) -> ConfiguredLogDestination {
    let catalog = LogModuleCatalog::new(vec![LogModuleRegistration::new(
        "recovery-test",
        LogCapabilities::new(vec![LogRecordType::Audit]).unwrap(),
        Box::new(CountingFactory { deliveries }),
    )])
    .unwrap();
    catalog
        .create_destination(
            &LogModuleIdentifier::new("recovery-test").unwrap(),
            &TrustedLogModuleContext::from_server_authority(
                authority,
                PathBuf::from("/unused"),
                [9; 16],
            ),
        )
        .unwrap()
}

#[test]
fn terminal_projection_round_trips_every_immutable_field_and_exact_binding() {
    let (authority, issuer) = authority_and_issuer();
    let record = terminal_record(&issuer);
    let retained_binding = binding(&authority, 7, 3);
    let projection = AuditTerminalRecoveryProjection::capture(&record, &retained_binding).unwrap();
    let restored = AuditTerminalRecoveryProjection::from_persisted(projection.as_bytes().to_vec())
        .unwrap()
        .restore(&issuer)
        .unwrap();

    assert_eq!(restored.binding(), &retained_binding);
    let LogRecordPersistenceView::Audit(original) = record.persistence_view() else {
        panic!("fixture must be Audit");
    };
    let LogRecordPersistenceView::Audit(replayed) = restored.record().persistence_view() else {
        panic!("restored record must be Audit");
    };
    assert_eq!(replayed.record_id(), original.record_id());
    assert_eq!(replayed.event_time(), original.event_time());
    assert_eq!(replayed.phase().as_str(), original.phase().as_str());
    assert_eq!(replayed.phase().result(), original.phase().result());
    assert_eq!(
        replayed.phase().attempt_record_id().unwrap().as_bytes(),
        original.phase().attempt_record_id().unwrap().as_bytes()
    );
    assert_eq!(replayed.correlation_id(), original.correlation_id());
    assert_eq!(
        replayed.body().classification(),
        original.body().classification()
    );
    assert_eq!(replayed.body().principal_type(), AuditPrincipalType::Human);
    assert_eq!(replayed.body().principal(), original.body().principal());
    assert_eq!(
        replayed.body().responsible_owner(),
        original.body().responsible_owner()
    );
    assert_eq!(replayed.body().action(), original.body().action());
    assert_eq!(replayed.body().target(), original.body().target());
    assert_eq!(replayed.body().detail(), original.body().detail());
}

#[test]
fn every_declared_classification_round_trips_and_unknown_future_values_fail_closed() {
    let (authority, issuer) = authority_and_issuer();
    let retained_binding = binding(&authority, 7, 3);

    for classification in AuditLogClassification::ALL {
        let correlation = CorrelationId::new("classification-recovery").unwrap();
        let attempt = CompleteLogRecord::audit_attempt(
            issuer.issue([1; 16]).unwrap(),
            EventTime::from_unix_milliseconds(10),
            correlation,
            AuditLogBody::new(
                *classification,
                AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
                "classify",
                "account:ar-22222222222222222222222222222222",
                "classification round trip",
            )
            .unwrap(),
        )
        .unwrap();
        let terminal = CompleteLogRecord::audit_completion(
            issuer.issue([2; 16]).unwrap(),
            EventTime::from_unix_milliseconds(11),
            attempt.attempt_record_id().unwrap(),
            LogResult::Success,
            CorrelationId::new("classification-recovery").unwrap(),
            AuditLogBody::new(
                *classification,
                AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
                "classify",
                "account:ar-22222222222222222222222222222222",
                "classification round trip",
            )
            .unwrap(),
        )
        .unwrap();
        let restored = AuditTerminalRecoveryProjection::capture(&terminal, &retained_binding)
            .unwrap()
            .restore(&issuer)
            .unwrap();
        let LogRecordPersistenceView::Audit(view) = restored.record().persistence_view() else {
            panic!("restored record must be Audit");
        };
        assert_eq!(view.body().classification(), classification.as_str());
    }

    let projection =
        AuditTerminalRecoveryProjection::capture(&terminal_record(&issuer), &retained_binding)
            .unwrap();
    let encoded = String::from_utf8(projection.as_bytes().to_vec()).unwrap();
    let future = encoded.replace(
        AuditLogClassification::AuthenticationUserDisabled.as_str(),
        "authentication.future-classification.changed",
    );
    let error = AuditTerminalRecoveryProjection::from_persisted(future.into_bytes())
        .unwrap()
        .restore(&issuer)
        .unwrap_err();
    assert_eq!(error, AuditTerminalRecoveryError::InvalidProjection);
    assert_eq!(
        error.to_string(),
        "Audit terminal recovery projection is invalid"
    );
}

#[test]
fn replay_refuses_changed_binding_before_delivery_and_acknowledges_exact_match() {
    let (authority, issuer) = authority_and_issuer();
    let retained_binding = binding(&authority, 7, 3);
    let projection =
        AuditTerminalRecoveryProjection::capture(&terminal_record(&issuer), &retained_binding)
            .unwrap();
    let restored = projection.restore(&issuer).unwrap();
    let deliveries = Arc::new(AtomicUsize::new(0));
    let destination = destination(&authority, deliveries.clone());
    let changed_binding = binding(&authority, 7, 4);
    let transition = AuditDestinationBindingTransition::from_server_authority(
        &authority,
        &retained_binding,
        &changed_binding,
    )
    .unwrap();
    assert_eq!(transition.retained(), &retained_binding);
    assert_eq!(transition.replacement(), &changed_binding);
    assert_eq!(
        AuditDestinationBindingTransition::from_server_authority(
            &authority,
            &retained_binding,
            &retained_binding,
        )
        .unwrap_err(),
        AuditDestinationBindingTransitionError::UnchangedBinding
    );
    let changed_destination =
        ResolvedAuditDestination::from_server_authority(&authority, &changed_binding, &destination);

    assert_eq!(
        restored.deliver(&changed_destination).unwrap_err(),
        AuditTerminalReplayError::DestinationBindingChanged
    );
    assert_eq!(deliveries.load(Ordering::SeqCst), 0);

    let resolved_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &retained_binding,
        &destination,
    );
    let acknowledgement = restored.deliver(&resolved_destination).unwrap();
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
    assert!(acknowledgement.matches(restored.record().record_id().as_bytes(), &retained_binding));
    assert!(!acknowledgement.matches(&[8; 16], &retained_binding));

    let repeated_acknowledgement = restored.deliver(&resolved_destination).unwrap();
    assert_eq!(deliveries.load(Ordering::SeqCst), 2);
    assert!(
        repeated_acknowledgement
            .matches(restored.record().record_id().as_bytes(), &retained_binding)
    );
    assert_eq!(
        format!("{transition:?}"),
        "AuditDestinationBindingTransition(REDACTED)"
    );
}

#[test]
fn supersession_is_degraded_and_keeps_the_original_eligible_for_exact_late_delivery() {
    let (authority, issuer) = authority_and_issuer();
    let original_binding = binding(&authority, 7, 3);
    let replacement_binding = binding(&authority, 8, 1);
    let original =
        AuditTerminalRecoveryProjection::capture(&terminal_record(&issuer), &original_binding)
            .unwrap()
            .restore(&issuer)
            .unwrap();
    let transition = AuditDestinationBindingTransition::from_server_authority(
        &authority,
        &original_binding,
        &replacement_binding,
    )
    .unwrap();
    let authorization =
        AuditTerminalSupersessionAuthorization::from_server_authority(&authority, &original);
    let confirmation = AuditTerminalSupersessionConfirmation::from_server_authority(
        &authority,
        &original,
        &transition,
        &authorization,
    )
    .unwrap();
    let deliveries = Arc::new(AtomicUsize::new(0));
    let destination = destination(&authority, deliveries.clone());
    let replacement = ResolvedAuditDestination::from_server_authority(
        &authority,
        &replacement_binding,
        &destination,
    );
    let replacement = replacement.preflight_for_terminal_supersession().unwrap();
    let disposition = AuditTerminalSupersessionDisposition::capture(
        &original,
        &transition,
        &authorization,
        &confirmation,
        &replacement,
    )
    .unwrap();
    let imported =
        AuditTerminalSupersessionDisposition::from_persisted(disposition.as_bytes().to_vec())
            .unwrap();

    assert_eq!(
        imported.original_record_id(),
        original.record().record_id().as_bytes()
    );
    assert_eq!(imported.original_binding(), &original_binding);
    assert_eq!(imported.replacement_binding(), &replacement_binding);
    assert_eq!(
        imported.reason(),
        AuditTerminalSupersessionReason::DestinationPermanentlyUnavailable
    );
    assert_eq!(
        imported.reason().as_str(),
        "destination_permanently_unavailable"
    );
    assert_eq!(imported.completeness(), AuditTerminalCompleteness::Degraded);
    assert_eq!(imported.completeness().as_str(), "degraded");
    assert_eq!(
        imported.original_state(),
        SupersededAuditTerminalState::RetainedPendingLateDelivery
    );

    let wrong_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &replacement_binding,
        &destination,
    );
    assert_eq!(
        original.deliver(&wrong_destination).unwrap_err(),
        AuditTerminalReplayError::DestinationBindingChanged
    );
    let retained_destination = ResolvedAuditDestination::from_server_authority(
        &authority,
        &original_binding,
        &destination,
    );
    assert!(original.deliver(&retained_destination).is_ok());
    assert_eq!(deliveries.load(Ordering::SeqCst), 1);
    assert_eq!(
        format!("{imported:?}"),
        "AuditTerminalSupersessionDisposition(REDACTED)"
    );
}

#[test]
fn malformed_or_secret_bearing_supersession_dispositions_fail_closed() {
    let (authority, issuer) = authority_and_issuer();
    let original_binding = binding(&authority, 7, 3);
    let replacement_binding = binding(&authority, 8, 1);
    let original =
        AuditTerminalRecoveryProjection::capture(&terminal_record(&issuer), &original_binding)
            .unwrap()
            .restore(&issuer)
            .unwrap();
    let transition = AuditDestinationBindingTransition::from_server_authority(
        &authority,
        &original_binding,
        &replacement_binding,
    )
    .unwrap();
    let authorization =
        AuditTerminalSupersessionAuthorization::from_server_authority(&authority, &original);
    let confirmation = AuditTerminalSupersessionConfirmation::from_server_authority(
        &authority,
        &original,
        &transition,
        &authorization,
    )
    .unwrap();
    let destination = destination(&authority, Arc::new(AtomicUsize::new(0)));
    let replacement = ResolvedAuditDestination::from_server_authority(
        &authority,
        &replacement_binding,
        &destination,
    );
    let replacement = replacement.preflight_for_terminal_supersession().unwrap();
    let disposition = AuditTerminalSupersessionDisposition::capture(
        &original,
        &transition,
        &authorization,
        &confirmation,
        &replacement,
    )
    .unwrap();
    let wrong_binding = binding(&authority, 9, 1);
    let wrong_replacement =
        ResolvedAuditDestination::from_server_authority(&authority, &wrong_binding, &destination)
            .preflight_for_terminal_supersession()
            .unwrap();
    assert_eq!(
        AuditTerminalSupersessionDisposition::capture(
            &original,
            &transition,
            &authorization,
            &confirmation,
            &wrong_replacement,
        )
        .unwrap_err(),
        AuditTerminalSupersessionError::EvidenceMismatch
    );
    let document: serde_json::Value = serde_json::from_slice(disposition.as_bytes()).unwrap();

    let mut rewritten_reason = document.clone();
    rewritten_reason["reason"] = serde_json::Value::String("correction".to_owned());
    let mut false_completeness = document.clone();
    false_completeness["completeness"] = serde_json::Value::String("complete".to_owned());
    let mut false_acknowledgement = document.clone();
    false_acknowledgement["original_state"] = serde_json::Value::String("acknowledged".to_owned());
    let mut secret_bearing = document;
    secret_bearing["raw_error"] =
        serde_json::Value::String("temporary-password=do-not-log".to_owned());

    for invalid in [
        rewritten_reason,
        false_completeness,
        false_acknowledgement,
        secret_bearing,
    ] {
        let error = AuditTerminalSupersessionDisposition::from_persisted(
            serde_json::to_vec(&invalid).unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, AuditTerminalSupersessionError::InvalidDisposition);
        assert_eq!(
            error.to_string(),
            "Audit terminal supersession disposition is invalid"
        );
        assert!(!error.to_string().contains("temporary-password"));
    }
    assert_eq!(
        AuditTerminalSupersessionDisposition::from_persisted(vec![
            b'x';
            MAX_AUDIT_TERMINAL_SUPERSESSION_BYTES
                + 1
        ])
        .unwrap_err(),
        AuditTerminalSupersessionError::InvalidDisposition
    );
}

#[test]
fn correction_failure_with_automation_owner_round_trips() {
    let (authority, issuer) = authority_and_issuer();
    let correlation = CorrelationId::new("automation-recovery").unwrap();
    let principal = || {
        AuditPrincipal::automation(
            "automation:nightly-reconciliation",
            "account:ar-11111111111111111111111111111111",
        )
        .unwrap()
    };
    let attempt = CompleteLogRecord::audit_attempt(
        issuer.issue([3; 16]).unwrap(),
        EventTime::from_unix_milliseconds(20),
        correlation,
        AuditLogBody::new(
            AuditLogClassification::ProviderOperationStarted,
            principal(),
            "operation-start",
            "operation:reconcile-tickets",
            "provider operation accepted",
        )
        .unwrap(),
    )
    .unwrap();
    let correction = CompleteLogRecord::audit_correction(
        issuer.issue([4; 16]).unwrap(),
        EventTime::from_unix_milliseconds(21),
        attempt.attempt_record_id().unwrap(),
        LogResult::Failure,
        CorrelationId::new("automation-recovery").unwrap(),
        AuditLogBody::new(
            AuditLogClassification::ProviderOperationCompleted,
            principal(),
            "operation-complete",
            "operation:reconcile-tickets",
            "corrected outcome: provider operation failed",
        )
        .unwrap(),
    )
    .unwrap();

    let restored =
        AuditTerminalRecoveryProjection::capture(&correction, &binding(&authority, 7, 3))
            .unwrap()
            .restore(&issuer)
            .unwrap();
    let LogRecordPersistenceView::Audit(view) = restored.record().persistence_view() else {
        panic!("restored record must be Audit");
    };
    assert_eq!(view.phase().as_str(), "correction");
    assert_eq!(view.phase().result(), Some(LogResult::Failure));
    assert_eq!(view.body().principal_type(), AuditPrincipalType::Automation);
    assert_eq!(
        view.body().responsible_owner(),
        Some("account:ar-11111111111111111111111111111111")
    );
}

#[test]
fn maximal_valid_escaped_record_fits_the_derived_projection_bound() {
    let (authority, issuer) = authority_and_issuer();
    let correlation = "\0".repeat(64);
    let principal = || AuditPrincipal::automation("\0".repeat(256), "\0".repeat(256)).unwrap();
    let body = || {
        AuditLogBody::new(
            AuditLogClassification::AuthenticationMfaModuleEnablementChanged,
            principal(),
            "\0".repeat(128),
            "\0".repeat(1024),
            "\0".repeat(4096),
        )
        .unwrap()
    };
    let attempt = CompleteLogRecord::audit_attempt(
        issuer.issue([5; 16]).unwrap(),
        EventTime::from_unix_milliseconds(u64::MAX - 1),
        CorrelationId::new(correlation.clone()).unwrap(),
        body(),
    )
    .unwrap();
    let terminal = CompleteLogRecord::audit_completion(
        issuer.issue([6; 16]).unwrap(),
        EventTime::from_unix_milliseconds(u64::MAX),
        attempt.attempt_record_id().unwrap(),
        LogResult::Failure,
        CorrelationId::new(correlation).unwrap(),
        body(),
    )
    .unwrap();

    let projection =
        AuditTerminalRecoveryProjection::capture(&terminal, &binding(&authority, 255, u64::MAX))
            .unwrap();
    assert!(projection.as_bytes().len() > 16 * 1024);
    assert!(projection.as_bytes().len() <= MAX_AUDIT_TERMINAL_RECOVERY_BYTES);
    projection.restore(&issuer).unwrap();
}

#[test]
fn forged_owner_and_zero_identifiers_or_binding_values_are_rejected() {
    let (authority, issuer) = authority_and_issuer();
    let projection = AuditTerminalRecoveryProjection::capture(
        &terminal_record(&issuer),
        &binding(&authority, 7, 3),
    )
    .unwrap();
    let document: serde_json::Value = serde_json::from_slice(projection.as_bytes()).unwrap();
    let zero_identifier = serde_json::Value::Array(vec![serde_json::Value::from(0); 16]);

    let mut forged_owner = document.clone();
    forged_owner["record"]["responsible_owner"] =
        serde_json::Value::String("account:ar-ffffffffffffffffffffffffffffffff".to_owned());

    let mut zero_record = document.clone();
    zero_record["record"]["record_id"] = zero_identifier.clone();

    let mut zero_attempt = document.clone();
    zero_attempt["record"]["attempt_record_id"] = zero_identifier.clone();

    let mut zero_binding = document.clone();
    zero_binding["binding"]["identifier"] = zero_identifier;

    let mut zero_binding_version = document;
    zero_binding_version["binding"]["version"] = serde_json::Value::from(0);

    for invalid in [
        forged_owner,
        zero_record,
        zero_attempt,
        zero_binding,
        zero_binding_version,
    ] {
        let invalid =
            AuditTerminalRecoveryProjection::from_persisted(serde_json::to_vec(&invalid).unwrap())
                .unwrap();
        let error = invalid.restore(&issuer).unwrap_err();
        assert_eq!(error, AuditTerminalRecoveryError::InvalidProjection);
        assert_eq!(
            error.to_string(),
            "Audit terminal recovery projection is invalid"
        );
        assert_eq!(
            format!("{invalid:?}"),
            "AuditTerminalRecoveryProjection(REDACTED)"
        );
    }
}

#[test]
fn attempts_malformed_and_oversized_projections_are_rejected_payload_free() {
    let (authority, issuer) = authority_and_issuer();
    let attempt = CompleteLogRecord::audit_attempt(
        issuer.issue([1; 16]).unwrap(),
        EventTime::from_unix_milliseconds(10),
        CorrelationId::new("secret-correlation").unwrap(),
        AuditLogBody::new(
            AuditLogClassification::AuthenticationUserDisabled,
            AuditPrincipal::human("account:ar-11111111111111111111111111111111").unwrap(),
            "disable",
            "account:ar-22222222222222222222222222222222",
            "accountable action accepted",
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        AuditTerminalRecoveryProjection::capture(&attempt, &binding(&authority, 7, 3)).unwrap_err(),
        AuditTerminalRecoveryError::NotTerminalAudit
    );
    assert_eq!(
        AuditTerminalRecoveryProjection::from_persisted(vec![
            b'x';
            MAX_AUDIT_TERMINAL_RECOVERY_BYTES + 1
        ])
        .unwrap_err(),
        AuditTerminalRecoveryError::InvalidProjection
    );
    let malformed = AuditTerminalRecoveryProjection::from_persisted(
        br#"{"version":1,"secret":"temporary-password"}"#.to_vec(),
    )
    .unwrap();
    let error = malformed.restore(&issuer).unwrap_err();
    assert_eq!(error, AuditTerminalRecoveryError::InvalidProjection);
    assert!(!error.to_string().contains("temporary-password"));
    assert_eq!(
        format!("{malformed:?}"),
        "AuditTerminalRecoveryProjection(REDACTED)"
    );
}

struct CountingFactory {
    deliveries: Arc<AtomicUsize>,
}

impl LogDestinationFactory for CountingFactory {
    fn accepted_settings(&self) -> LogSettingsContract {
        LogSettingsContract::none()
    }

    fn create(
        &self,
        _context: &LogModuleFactoryContext<'_>,
    ) -> Result<Box<dyn LogDestination>, LogDestinationError> {
        Ok(Box::new(CountingDestination {
            deliveries: self.deliveries.clone(),
        }))
    }
}

struct CountingDestination {
    deliveries: Arc<AtomicUsize>,
}

impl LogDestination for CountingDestination {
    fn deliver(
        &self,
        _record: &CompleteLogRecord,
        acknowledgement: DurableAcknowledgement,
    ) -> Result<DurableAcknowledgement, LogDestinationError> {
        self.deliveries.fetch_add(1, Ordering::SeqCst);
        Ok(acknowledgement)
    }

    fn preflight(&self, _record_type: LogRecordType) -> Result<(), LogDestinationError> {
        Ok(())
    }
}
