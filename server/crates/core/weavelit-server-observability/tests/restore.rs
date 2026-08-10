//! Restore completion result construction.

use weavelit_server_database::{
    DEPLOYMENT_IDENTIFIER_LENGTH, DeploymentIdentifier, STATE_IDENTIFIER_LENGTH, StateIdentifier,
    WorkflowKind,
};
use weavelit_server_log::TrustedRecordIssuer;
use weavelit_server_log_authority::ServerLogAuthority;
use weavelit_server_observability::{ObservabilityError, ServerObservability};

/// Builds the producer the Server composition root would supply.
fn observability() -> ServerObservability {
    ServerObservability::new(TrustedRecordIssuer::from_server_authority(
        &ServerLogAuthority::new(),
    ))
}

fn deployment() -> DeploymentIdentifier {
    DeploymentIdentifier::from_bytes([7; DEPLOYMENT_IDENTIFIER_LENGTH])
        .expect("a nonzero deployment identifier is valid")
}

fn record_identifier() -> StateIdentifier {
    StateIdentifier::from_bytes([3; STATE_IDENTIFIER_LENGTH])
        .expect("a nonzero state identifier is valid")
}

#[test]
fn a_restore_completion_binds_its_record_and_obligation_to_one_identity() {
    let prepared = observability()
        .prepare_restore_completion(
            record_identifier(),
            deployment(),
            1_760_000_000_000,
            "corr-1",
        )
        .expect("a well-formed restore completion must prepare");

    let obligation = prepared.obligation();
    assert_eq!(obligation.workflow(), WorkflowKind::Restore);
    assert_eq!(obligation.record_identifier(), record_identifier());
    assert_eq!(obligation.classification().as_str(), "lifecycle.restore");
    assert_eq!(obligation.correlation_identifier().as_str(), "corr-1");
    assert_eq!(obligation.event_time_milliseconds(), 1_760_000_000_000);
}

#[test]
fn the_detail_names_the_replacement_deployment_and_nothing_else() {
    let prepared = observability()
        .prepare_restore_completion(record_identifier(), deployment(), 1, "corr-1")
        .expect("a well-formed restore completion must prepare");

    assert_eq!(
        prepared.obligation().detail().as_str(),
        "restore completed for deployment 07070707070707070707070707070707"
    );
}

#[test]
fn a_negative_event_time_is_rejected() {
    let error = observability()
        .prepare_restore_completion(record_identifier(), deployment(), -1, "corr-1")
        .expect_err("a negative event time must be rejected");

    assert_eq!(error, ObservabilityError::InvalidEventTime);
}

#[test]
fn an_empty_correlation_identifier_is_rejected() {
    let error = observability()
        .prepare_restore_completion(record_identifier(), deployment(), 1, "")
        .expect_err("an empty correlation identifier must be rejected");

    assert_eq!(error, ObservabilityError::InvalidCompletionObligation);
}

#[test]
fn no_rendered_error_discloses_event_content() {
    for error in [
        ObservabilityError::InvalidEventTime,
        ObservabilityError::InvalidCompletionObligation,
        ObservabilityError::InvalidLogRecord,
    ] {
        let rendered = format!("{error} {error:?}");
        assert!(!rendered.contains("07070707"), "{rendered}");
        assert!(!rendered.contains("corr-1"), "{rendered}");
    }
}
