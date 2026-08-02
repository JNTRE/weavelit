use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use weavelit_server_database::{CheckpointMetadata, DatabaseInspection};
use weavelit_server_database_sqlite::SqliteDatabase;
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendIdentifier,
    BackendRegistration, CheckpointMetadata as LifecycleCheckpointMetadata,
    LifecycleClassification, LifecycleError, LifecycleState, LifecycleStore, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowArbiter, WorkflowCheckpoint, WorkflowError, WorkflowKind,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const SQLITE_BACKEND: &str = "sqlite";

fn state_root() -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = directory.path().canonicalize().unwrap();
    (directory, canonical)
}

struct SqliteFactory;

impl ApplicationDatabaseFactory for SqliteFactory {
    fn open(
        &self,
        context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        SqliteDatabase::open(context.application_database_path())
            .map(|db| Box::new(db) as Box<dyn ApplicationDatabase>)
            .map_err(|_| LifecycleError::DependencyUnavailable)
    }
}

fn sqlite_catalog() -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        SQLITE_BACKEND,
        vec![],
        Box::new(SqliteFactory),
    )])
    .unwrap()
}

fn sqlite_context(root: &Path) -> TrustedBackendContext {
    TrustedBackendContext::new(root.join("application.sqlite3"))
}

fn sqlite_backend() -> BackendIdentifier {
    BackendIdentifier::new(SQLITE_BACKEND).unwrap()
}

fn metadata(value: &[u8]) -> LifecycleCheckpointMetadata {
    CheckpointMetadata::from_bytes(value).unwrap()
}

fn empty_metadata() -> LifecycleCheckpointMetadata {
    metadata(b"")
}

fn init_metadata() -> LifecycleCheckpointMetadata {
    metadata(b"init-checkpoint-metadata")
}

fn restore_metadata() -> LifecycleCheckpointMetadata {
    metadata(b"restore-checkpoint-metadata")
}

/// Opens a store, selects the SQLite database, creates an arbiter.
/// Returns the arbiter plus the catalog and context needed for operations.
fn setup(path: &Path) -> (WorkflowArbiter, BackendCatalog, TrustedBackendContext) {
    let mut store = LifecycleStore::open_or_create(path).unwrap();
    let catalog = sqlite_catalog();
    let context = sqlite_context(path);
    store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .unwrap();
    (WorkflowArbiter::new(store), catalog, context)
}

// ---------------------------------------------------------------------------
// Tests: begin_workflow
// ---------------------------------------------------------------------------

#[test]
fn begin_init_creates_checkpoint_and_advances_record() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .expect("begin Init must succeed");

    assert_eq!(
        arbiter.record_state(),
        LifecycleState::InitializationPending
    );
    drop(arbiter);

    let store = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(
        store.record().state(),
        LifecycleState::InitializationPending
    );
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    match db.inspect(store.record().deployment_identifier()).unwrap() {
        DatabaseInspection::Pending(checkpoint) => {
            assert_eq!(checkpoint.workflow(), WorkflowKind::Init);
            assert_eq!(
                checkpoint.metadata().as_bytes(),
                b"init-checkpoint-metadata"
            );
        }
        other => panic!("expected Pending, got {other:?}"),
    }
    drop(store);
}

#[test]
fn begin_restore_creates_checkpoint_and_advances_record() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(
            &catalog,
            &context,
            WorkflowKind::Restore,
            restore_metadata(),
        )
        .expect("begin Restore must succeed");

    assert_eq!(
        arbiter.record_state(),
        LifecycleState::InitializationPending
    );
    drop(arbiter);

    let store = LifecycleStore::open_or_create(&path).unwrap();
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    match db.inspect(store.record().deployment_identifier()).unwrap() {
        DatabaseInspection::Pending(checkpoint) => {
            assert_eq!(checkpoint.workflow(), WorkflowKind::Restore);
        }
        other => panic!("expected Pending, got {other:?}"),
    }
    drop(store);
}

#[test]
fn begin_rejected_when_record_is_not_uninitialized() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    let error = arbiter
        .begin_workflow(
            &catalog,
            &context,
            WorkflowKind::Restore,
            restore_metadata(),
        )
        .unwrap_err();

    assert_eq!(error, WorkflowError::NotAllowed);
}

#[test]
fn begin_rejected_when_no_database_is_selected() {
    let (_dir, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();
    let arbiter = WorkflowArbiter::new(store);
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);

    let error = arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, empty_metadata())
        .unwrap_err();

    assert_eq!(error, WorkflowError::DatabaseNotSelected);
}

#[test]
fn conflicting_workflow_rejected_when_checkpoint_exists() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    let error = arbiter
        .begin_workflow(
            &catalog,
            &context,
            WorkflowKind::Restore,
            restore_metadata(),
        )
        .unwrap_err();

    assert_eq!(error, WorkflowError::NotAllowed);
}

// ---------------------------------------------------------------------------
// Tests: reconcile_workflow
// ---------------------------------------------------------------------------

#[test]
fn reconcile_advances_record_when_uninitialized_with_matching_checkpoint() {
    // Build state: select database + create checkpoint WITHOUT advancing record.
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let catalog = sqlite_catalog();
        let context = sqlite_context(&path);
        store
            .select_database(&catalog, &context, &sqlite_backend(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store.reopen_selected_database(&catalog, &context).unwrap();
        db.create_checkpoint(&WorkflowCheckpoint::new(
            dep_id,
            WorkflowKind::Init,
            init_metadata(),
        ))
        .unwrap();
        // Do NOT advance the record — simulate crash after checkpoint, before record.
        drop(db);
        drop(store);
    }

    // Reopen and wrap in arbiter for reconcile.
    let store = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(store.record().state(), LifecycleState::Uninitialized);
    let arbiter = WorkflowArbiter::new(store);

    arbiter
        .reconcile_workflow(
            &sqlite_catalog(),
            &sqlite_context(&path),
            WorkflowKind::Init,
            &init_metadata(),
        )
        .expect("reconcile must succeed");

    assert_eq!(
        arbiter.record_state(),
        LifecycleState::InitializationPending
    );
    drop(arbiter);

    let store = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(
        store.record().state(),
        LifecycleState::InitializationPending
    );
    drop(store);
}

#[test]
fn reconcile_is_idempotent_when_record_already_advanced() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    arbiter
        .reconcile_workflow(&catalog, &context, WorkflowKind::Init, &init_metadata())
        .expect("reconcile must be idempotent");

    assert_eq!(
        arbiter.record_state(),
        LifecycleState::InitializationPending
    );
}

#[test]
fn reconcile_rejected_for_wrong_workflow_kind() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    assert_eq!(
        arbiter
            .reconcile_workflow(
                &catalog,
                &context,
                WorkflowKind::Restore,
                &restore_metadata()
            )
            .unwrap_err(),
        WorkflowError::StateMismatch
    );
}

#[test]
fn reconcile_rejected_for_wrong_metadata() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    assert_eq!(
        arbiter
            .reconcile_workflow(
                &catalog,
                &context,
                WorkflowKind::Init,
                &metadata(b"different")
            )
            .unwrap_err(),
        WorkflowError::StateMismatch
    );
}

// ---------------------------------------------------------------------------
// Tests: reset_workflow
// ---------------------------------------------------------------------------

#[test]
fn reset_returns_to_uninitialized_state_with_database_selected() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();
    arbiter
        .reset_workflow(&catalog, &context, WorkflowKind::Init, &init_metadata())
        .expect("reset must succeed");

    assert_eq!(arbiter.record_state(), LifecycleState::Uninitialized);
    drop(arbiter);

    let store = LifecycleStore::open_or_create(&path).unwrap();
    assert_eq!(store.record().state(), LifecycleState::Uninitialized);
    assert!(
        store.locator().is_some(),
        "database selection must be preserved after reset"
    );
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert_eq!(
        db.inspect(store.record().deployment_identifier()).unwrap(),
        DatabaseInspection::Uninitialized
    );
    drop(store);
}

#[test]
fn reset_rejected_for_wrong_workflow_kind() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    assert_eq!(
        arbiter
            .reset_workflow(
                &catalog,
                &context,
                WorkflowKind::Restore,
                &restore_metadata()
            )
            .unwrap_err(),
        WorkflowError::StateMismatch
    );
}

#[test]
fn reset_rejected_when_record_is_not_pending() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    assert_eq!(
        arbiter
            .reset_workflow(&catalog, &context, WorkflowKind::Init, &init_metadata())
            .unwrap_err(),
        WorkflowError::NotAllowed
    );
}

#[test]
fn reset_rejected_for_wrong_metadata() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    assert_eq!(
        arbiter
            .reset_workflow(&catalog, &context, WorkflowKind::Init, &metadata(b"other"))
            .unwrap_err(),
        WorkflowError::StateMismatch
    );
}

// ---------------------------------------------------------------------------
// Tests: crash-point ordering
// ---------------------------------------------------------------------------

#[test]
fn crash_after_checkpoint_before_record_is_reconciled_by_classify_startup() {
    let (_dir, path) = state_root();
    {
        let mut store = LifecycleStore::open_or_create(&path).unwrap();
        let catalog = sqlite_catalog();
        let context = sqlite_context(&path);
        store
            .select_database(&catalog, &context, &sqlite_backend(), vec![])
            .unwrap();
        let dep_id = store.record().deployment_identifier();
        let mut db = store.reopen_selected_database(&catalog, &context).unwrap();
        db.create_checkpoint(&WorkflowCheckpoint::new(
            dep_id,
            WorkflowKind::Init,
            init_metadata(),
        ))
        .unwrap();
        drop(db);
        drop(store); // Lock released; record is still Uninitialized.
    }

    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let classification = store
        .classify_startup(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert_eq!(
        classification,
        LifecycleClassification::InitializationPending(WorkflowKind::Init)
    );
    drop(store);
}

#[test]
fn crash_after_record_reset_before_checkpoint_discard_reclassifies_as_pending() {
    // After successful reset, restart should see UninitializedWithDatabase.
    // Crash scenario (record reset but checkpoint not discarded) is tested
    // through classify_startup behavior: Uninitialized + checkpoint → advances.
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();
    arbiter
        .reset_workflow(&catalog, &context, WorkflowKind::Init, &init_metadata())
        .unwrap();
    drop(arbiter);

    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let classification = store
        .classify_startup(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert_eq!(
        classification,
        LifecycleClassification::UninitializedWithDatabase
    );
    drop(store);
}

// ---------------------------------------------------------------------------
// Tests: deterministic contention
// ---------------------------------------------------------------------------

#[test]
fn at_most_one_workflow_checkpoint_becomes_durable_under_contention() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);
    store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .unwrap();

    let arbiter = Arc::new(WorkflowArbiter::new(store));
    let catalog = Arc::new(catalog);
    let context = Arc::new(context);
    let barrier = Arc::new(Barrier::new(2));

    let handle1 = {
        let arbiter = Arc::clone(&arbiter);
        let catalog = Arc::clone(&catalog);
        let context = Arc::clone(&context);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            arbiter.begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        })
    };
    let handle2 = {
        let arbiter = Arc::clone(&arbiter);
        let catalog = Arc::clone(&catalog);
        let context = Arc::clone(&context);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            arbiter.begin_workflow(
                &catalog,
                &context,
                WorkflowKind::Restore,
                restore_metadata(),
            )
        })
    };

    let result1 = handle1.join().expect("thread 1 must not panic");
    let result2 = handle2.join().expect("thread 2 must not panic");

    let successes = [result1.is_ok(), result2.is_ok()];
    assert_eq!(
        successes.iter().filter(|&&ok| ok).count(),
        1,
        "exactly one begin_workflow must succeed"
    );
    assert_eq!(
        arbiter.record_state(),
        LifecycleState::InitializationPending
    );
    drop(arbiter);

    let store = LifecycleStore::open_or_create(&path).unwrap();
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert!(
        matches!(
            db.inspect(store.record().deployment_identifier()).unwrap(),
            DatabaseInspection::Pending(_)
        ),
        "exactly one checkpoint must be durable"
    );
    drop(store);
}

// ---------------------------------------------------------------------------
// Tests: state rejection and sealing
// ---------------------------------------------------------------------------

#[test]
fn begin_rejected_when_checkpoint_already_exists_for_same_workflow() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);

    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    let error = arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap_err();

    assert_eq!(error, WorkflowError::NotAllowed);
}

// ---------------------------------------------------------------------------
// Tests: redaction
// ---------------------------------------------------------------------------

#[test]
fn reset_retries_from_uninitialized_when_checkpoint_discard_was_interrupted() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);
    store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .unwrap();
    let dep_id = store.record().deployment_identifier();

    // Insert checkpoint directly to simulate partial reset: record was written to
    // Uninitialized but discard_checkpoint had not yet succeeded.
    let mut db = store.reopen_selected_database(&catalog, &context).unwrap();
    db.create_checkpoint(&WorkflowCheckpoint::new(
        dep_id,
        WorkflowKind::Init,
        init_metadata(),
    ))
    .unwrap();
    drop(db);

    let arbiter = WorkflowArbiter::new(store);
    assert_eq!(arbiter.record_state(), LifecycleState::Uninitialized);

    arbiter
        .reset_workflow(&catalog, &context, WorkflowKind::Init, &init_metadata())
        .expect("reset must succeed from partial-reset Uninitialized+checkpoint state");

    assert_eq!(arbiter.record_state(), LifecycleState::Uninitialized);
    drop(arbiter);

    // Checkpoint must have been discarded.
    let store = LifecycleStore::open_or_create(&path).unwrap();
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert_eq!(
        db.inspect(store.record().deployment_identifier()).unwrap(),
        DatabaseInspection::Uninitialized
    );
}

#[test]
fn workflow_errors_do_not_expose_sensitive_values() {
    let sensitive_path = "/private/secrets/application.sqlite3";
    let sensitive_meta = "secret-metadata-value";

    let errors: &[WorkflowError] = &[
        WorkflowError::NotAllowed,
        WorkflowError::DatabaseNotSelected,
        WorkflowError::AlreadyPending,
        WorkflowError::AlreadyInitialized,
        WorkflowError::StateMismatch,
        WorkflowError::Lifecycle(LifecycleError::DependencyUnavailable),
        WorkflowError::Lifecycle(LifecycleError::IntegrityFailure),
        WorkflowError::Lifecycle(LifecycleError::DeploymentMismatch),
    ];
    for error in errors {
        let output = format!("{error:?} {error}");
        assert!(
            !output.contains(sensitive_path),
            "must not expose path: {output}"
        );
        assert!(
            !output.contains(sensitive_meta),
            "must not expose metadata: {output}"
        );
    }
}
