use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use weavelit_server::{StartupError, StartupOutcome, classify_restricted_startup, sqlite_catalog};
use weavelit_server_lifecycle::{
    BackendIdentifier, CheckpointMetadata, DeploymentIdentifier, LifecycleStore,
    TrustedBackendContext, WorkflowCheckpoint, WorkflowKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn state_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = directory.path().canonicalize().unwrap();
    (directory, canonical)
}

fn context(root: &std::path::Path) -> TrustedBackendContext {
    TrustedBackendContext::new(root.join("application.sqlite3"))
}

fn sqlite() -> BackendIdentifier {
    BackendIdentifier::new("sqlite").unwrap()
}

fn checkpoint(dep_id: DeploymentIdentifier, kind: WorkflowKind) -> WorkflowCheckpoint {
    WorkflowCheckpoint::new(
        dep_id,
        kind,
        CheckpointMetadata::from_bytes(b"meta".as_slice()).unwrap(),
    )
}

// ---------------------------------------------------------------------------
// Tests: fresh startup and restart
// ---------------------------------------------------------------------------

#[test]
fn fresh_first_start_classifies_as_uninitialized_without_database() {
    let (_dir, path) = state_root();
    let outcome = classify_restricted_startup(&path).expect("first start must succeed");
    assert_eq!(outcome, StartupOutcome::UninitializedWithoutDatabase);
}

#[test]
fn restart_without_selection_is_stable() {
    let (_dir, path) = state_root();
    classify_restricted_startup(&path).unwrap();
    let outcome = classify_restricted_startup(&path).expect("restart must succeed");
    assert_eq!(outcome, StartupOutcome::UninitializedWithoutDatabase);
}

#[test]
fn restart_with_selected_database_classifies_as_uninitialized_with_database() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
    }
    let outcome = classify_restricted_startup(&path).expect("restart with selection must succeed");
    assert_eq!(outcome, StartupOutcome::UninitializedWithDatabase);
}

#[test]
fn restart_with_init_pending_classifies_as_initialization_pending() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store
            .reopen_selected_database(&sqlite_catalog(), &context(&path))
            .unwrap();
        db.create_checkpoint(&checkpoint(dep_id, WorkflowKind::Init))
            .unwrap();
        drop(db);
    }
    let outcome = classify_restricted_startup(&path).expect("init-pending restart must succeed");
    assert_eq!(
        outcome,
        StartupOutcome::InitializationPending(WorkflowKind::Init)
    );
}

#[test]
fn restart_with_restore_pending_classifies_as_initialization_pending() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        store
            .select_database(&sqlite_catalog(), &context(&path), &sqlite(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store
            .reopen_selected_database(&sqlite_catalog(), &context(&path))
            .unwrap();
        db.create_checkpoint(&checkpoint(dep_id, WorkflowKind::Restore))
            .unwrap();
        drop(db);
    }
    let outcome = classify_restricted_startup(&path).expect("restore-pending restart must succeed");
    assert_eq!(
        outcome,
        StartupOutcome::InitializationPending(WorkflowKind::Restore)
    );
}

// ---------------------------------------------------------------------------
// Tests: fail-closed retained-state failures
// ---------------------------------------------------------------------------

#[test]
fn corrupted_anchor_fails_closed_with_anchor_set_invalid() {
    let (_dir, path) = state_root();
    classify_restricted_startup(&path).unwrap();

    let record_path = path.join("deployment-record.json");
    let mut bytes = fs::read(&record_path).unwrap();
    let pos = bytes.len() - 4;
    bytes[pos] = if bytes[pos] == b'A' { b'B' } else { b'A' };
    fs::write(&record_path, bytes).unwrap();

    let error = classify_restricted_startup(&path).unwrap_err();
    assert_eq!(error, StartupError::AnchorSetInvalid);
    let (category, _) = error.category_reason();
    assert_eq!(category, "storage_integrity_failure");
}

#[test]
fn concurrent_startup_fails_closed_as_state_root_in_use() {
    let (_dir, path) = state_root();
    let _store = LifecycleStore::open_or_create(&path).unwrap();

    let error = classify_restricted_startup(&path).unwrap_err();
    assert_eq!(error, StartupError::StateRootInUse);
    let (category, reason) = error.category_reason();
    assert_eq!(category, "preoperational_unavailable");
    assert_eq!(reason, "state_root_in_use");
}

// ---------------------------------------------------------------------------
// Tests: error category/reason pairs are well-formed and redacted
// ---------------------------------------------------------------------------

#[test]
fn every_startup_error_has_well_formed_category_reason() {
    let errors = [
        StartupError::StateRootNotConfigured,
        StartupError::StateRootPathInvalid,
        StartupError::StateRootInUse,
        StartupError::StorageOperationFailed,
        StartupError::DatabaseUnavailable,
        StartupError::AnchorSetInvalid,
        StartupError::AnchorVersionUnsupported,
        StartupError::AnchorBindingInvalid,
        StartupError::DatabaseIntegrityFailure,
        StartupError::StateCombinationInvalid,
    ];
    let sensitive = "/private/weavelit/state-root/sensitive";
    for error in &errors {
        let (category, reason) = error.category_reason();
        assert!(
            !category.is_empty(),
            "category must not be empty for {error:?}"
        );
        assert!(!reason.is_empty(), "reason must not be empty for {error:?}");
        assert!(!format!("{error:?}").contains(sensitive));
        assert!(!category.contains(sensitive));
        assert!(!reason.contains(sensitive));
    }
}
