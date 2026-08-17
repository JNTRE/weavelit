//! The approved Restore transfer bounds and single-operation permit.

use std::time::Duration;

use weavelit_server_restore::{
    MAX_AUTHENTICATED_PLAINTEXT_BYTES, MAX_CONCURRENT_RESTORE_OPERATIONS,
    MAX_ENCRYPTED_ARTIFACT_BYTES, RequestBudget, RestoreConcurrency, RestoreError,
    TOTAL_REQUEST_DEADLINE, TransferBounds, UPLOAD_DEADLINE, check_total_elapsed,
    check_upload_elapsed,
};

#[test]
fn the_approved_bounds_match_the_security_model() {
    assert_eq!(MAX_ENCRYPTED_ARTIFACT_BYTES, 256 * 1024 * 1024);
    assert_eq!(MAX_AUTHENTICATED_PLAINTEXT_BYTES, 256 * 1024 * 1024);
    assert_eq!(UPLOAD_DEADLINE, Duration::from_secs(120));
    assert_eq!(TOTAL_REQUEST_DEADLINE, Duration::from_secs(300));
    assert_eq!(MAX_CONCURRENT_RESTORE_OPERATIONS, 1);

    let bounds = TransferBounds::APPROVED;
    assert_eq!(bounds.max_encrypted_artifact_bytes, 256 * 1024 * 1024);
    assert_eq!(bounds.max_authenticated_plaintext_bytes, 256 * 1024 * 1024);
    assert_eq!(TransferBounds::default(), bounds);
}

#[test]
fn an_artifact_at_the_bound_is_accepted_and_one_byte_over_is_not() {
    let bounds = TransferBounds::APPROVED;
    assert_eq!(bounds.check_artifact(MAX_ENCRYPTED_ARTIFACT_BYTES), Ok(()));
    assert_eq!(
        bounds.check_artifact(MAX_ENCRYPTED_ARTIFACT_BYTES + 1),
        Err(RestoreError::BackupInvalid)
    );
}

#[test]
fn deadlines_fail_the_operation_rather_than_leaking_a_distinct_reason() {
    assert_eq!(check_upload_elapsed(UPLOAD_DEADLINE), Ok(()));
    assert_eq!(
        check_upload_elapsed(UPLOAD_DEADLINE + Duration::from_millis(1)),
        Err(RestoreError::RestoreFailed)
    );

    assert_eq!(check_total_elapsed(TOTAL_REQUEST_DEADLINE), Ok(()));
    assert_eq!(
        check_total_elapsed(TOTAL_REQUEST_DEADLINE + Duration::from_millis(1)),
        Err(RestoreError::RestoreFailed)
    );
}

#[test]
fn a_fresh_request_budget_is_within_the_total_deadline() {
    let budget = RequestBudget::start();
    assert!(budget.elapsed() < TOTAL_REQUEST_DEADLINE);
    assert_eq!(budget.check(), Ok(()));
}

#[test]
fn only_one_restore_operation_holds_the_permit_at_a_time() {
    let gate = RestoreConcurrency::new();

    let first = gate.try_acquire().expect("the first operation acquires");
    assert_eq!(
        gate.try_acquire().err(),
        Some(RestoreError::RestorePending),
        "a concurrent operation is refused"
    );

    drop(first);
    gate.try_acquire()
        .expect("the permit is released when the operation ends");
}
