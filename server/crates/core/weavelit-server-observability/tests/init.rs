//! Init completion result construction.

use weavelit_server_database::{
    DEPLOYMENT_IDENTIFIER_LENGTH, DeploymentIdentifier, STATE_IDENTIFIER_LENGTH, StateIdentifier,
    WorkflowKind,
};
use weavelit_server_log::{CompleteLogRecord, TrustedRecordIssuer};
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
fn an_init_completion_binds_its_record_and_obligation_to_one_identity() {
    let prepared = observability()
        .prepare_init_completion(
            record_identifier(),
            deployment(),
            1_760_000_000_000,
            "corr-1",
        )
        .expect("a well-formed init completion must prepare");

    let obligation = prepared.obligation();
    assert_eq!(obligation.workflow(), WorkflowKind::Init);
    assert_eq!(obligation.record_identifier(), record_identifier());
    assert_eq!(obligation.classification().as_str(), "lifecycle.init");
    assert_eq!(obligation.correlation_identifier().as_str(), "corr-1");
    assert_eq!(obligation.event_time_milliseconds(), 1_760_000_000_000);
}

/// The record and the obligation must be built from the same fields.
///
/// They are delivered and persisted through different paths, so a divergence
/// between them would leave the System Log describing one event and the stored
/// acknowledgement obligation describing another.
#[test]
fn the_record_and_the_obligation_carry_the_same_event() {
    let prepared = observability()
        .prepare_init_completion(
            record_identifier(),
            deployment(),
            1_700_000_000_000,
            "corr-1",
        )
        .expect("a well-formed init completion must prepare");

    let obligation_detail = prepared.obligation().detail().as_str().to_owned();
    let obligation_time = prepared.obligation().event_time_milliseconds();
    let (record, obligation) = prepared.into_parts();

    assert!(!obligation_detail.is_empty());
    assert_eq!(obligation.detail().as_str(), obligation_detail);
    assert_eq!(obligation.event_time_milliseconds(), obligation_time);

    let CompleteLogRecord::System {
        event_time,
        correlation_id,
        body,
        ..
    } = record
    else {
        panic!("an Init completion result is a System Log record");
    };
    assert_eq!(correlation_id.as_str(), "corr-1");
    assert_eq!(
        event_time.unix_milliseconds(),
        u64::try_from(obligation_time).expect("a nonnegative event time")
    );
    assert_eq!(body.classification(), obligation.classification().as_str());
    assert_eq!(body.detail(), obligation_detail);
}

#[test]
fn the_detail_names_the_initialized_deployment_and_nothing_else() {
    let prepared = observability()
        .prepare_init_completion(record_identifier(), deployment(), 1, "corr-1")
        .expect("a well-formed init completion must prepare");

    assert_eq!(
        prepared.obligation().detail().as_str(),
        "initialization completed for deployment 07070707070707070707070707070707"
    );
}

#[test]
fn a_negative_event_time_is_rejected() {
    let error = observability()
        .prepare_init_completion(record_identifier(), deployment(), -1, "corr-1")
        .expect_err("a negative event time must be rejected");

    assert_eq!(error, ObservabilityError::InvalidEventTime);
}

#[test]
fn an_empty_correlation_identifier_is_rejected() {
    let error = observability()
        .prepare_init_completion(record_identifier(), deployment(), 1, "")
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
