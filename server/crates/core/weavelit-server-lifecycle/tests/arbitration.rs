use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    thread,
};

use weavelit_server_database::{CheckpointMetadata, DatabaseInspection};
use weavelit_server_database_sqlite::{RetainedSqliteInspection, SqliteDatabase};
use weavelit_server_lifecycle::{
    AnchorLoadState, ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog,
    BackendIdentifier, BackendRegistration, CheckpointMetadata as LifecycleCheckpointMetadata,
    LifecycleClassification, LifecycleError, LifecycleState, LifecycleStore,
    RetainedDatabaseInspection, SelectionError, SelectionFailureKind, TrustedBackendContext,
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

    fn inspect_retained(
        &self,
        context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
        expected_deployment_identifier: weavelit_server_lifecycle::DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        SqliteDatabase::inspect_retained(
            context.application_database_path(),
            expected_deployment_identifier,
        )
        .map(|inspection| match inspection {
            RetainedSqliteInspection::Inspected(inspection) => {
                RetainedDatabaseInspection::Inspected(inspection)
            }
            RetainedSqliteInspection::WalPresent => RetainedDatabaseInspection::RedeployRequired,
        })
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

/// Returns every retained locator filename with its exact bytes, sorted by name.
fn locator_files(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|entry| {
            entry
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("database-locator-"))
        })
        .map(|entry| {
            (
                entry.file_name().unwrap().to_str().unwrap().to_owned(),
                fs::read(&entry).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// Panics inside the permit so a test can observe poisoning rather than simulate it.
struct PanickingFactory;

impl ApplicationDatabaseFactory for PanickingFactory {
    fn open(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        panic!("factory panic under the lifecycle permit");
    }

    fn inspect_retained(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
        _expected_deployment_identifier: weavelit_server_lifecycle::DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        panic!("factory panic under the lifecycle permit");
    }
}

fn panicking_catalog() -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "panicking-backend",
        vec![],
        Box::new(PanickingFactory),
    )])
    .unwrap()
}

fn panicking_backend() -> BackendIdentifier {
    BackendIdentifier::new("panicking-backend").unwrap()
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
// Tests: crash-point ordering
// ---------------------------------------------------------------------------

#[test]
fn crash_after_checkpoint_before_record_requires_redeploy_at_startup() {
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

    let store = LifecycleStore::open_or_create(&path).unwrap();
    let classification = store
        .classify_startup(&sqlite_catalog(), &sqlite_context(&path))
        .unwrap();
    assert_eq!(
        classification,
        LifecycleClassification::Interrupted(
            weavelit_server_lifecycle::InterruptedLifecycleAction::RedeployNew
        )
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
// Tests: retained eligible state and redaction
// ---------------------------------------------------------------------------

#[test]
fn retained_eligible_store_can_begin_init_or_restore() {
    for (kind, metadata) in [
        (WorkflowKind::Init, init_metadata()),
        (WorkflowKind::Restore, restore_metadata()),
    ] {
        let (_dir, path) = state_root();
        {
            let mut store = LifecycleStore::open_or_create(&path).unwrap();
            store
                .select_database(
                    &sqlite_catalog(),
                    &sqlite_context(&path),
                    &sqlite_backend(),
                    vec![],
                )
                .unwrap();
        }

        let store = LifecycleStore::open_or_create(&path).unwrap();
        assert_eq!(store.load_state(), AnchorLoadState::Retained);
        let arbiter = WorkflowArbiter::new(store);

        arbiter
            .begin_workflow(&sqlite_catalog(), &sqlite_context(&path), kind, metadata)
            .expect("retained eligible store must permit a new workflow");
        assert_eq!(
            arbiter.record_state(),
            LifecycleState::InitializationPending
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: serialized database selection and live projection
// ---------------------------------------------------------------------------

#[test]
fn projection_reports_unselected_before_and_selected_after_selection() {
    let (_dir, path) = state_root();
    let arbiter = WorkflowArbiter::new(LifecycleStore::open_or_create(&path).unwrap());
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);

    assert!(
        !arbiter.projection().unwrap().database_selected(),
        "no database is selected before the first selection"
    );

    let (_database, projection) = arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("selection must succeed");

    assert!(
        projection.database_selected(),
        "the returned projection must reflect the state committed under the permit"
    );
    assert_eq!(arbiter.projection().unwrap(), projection);
}

#[test]
fn arbiter_exact_replay_leaves_the_locator_generation_and_bytes_unchanged() {
    let (_dir, path) = state_root();
    let arbiter = WorkflowArbiter::new(LifecycleStore::open_or_create(&path).unwrap());
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);

    arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("initial selection must succeed");
    let first_files = locator_files(&path);

    let (_database, projection) = arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("exact replay must succeed");

    assert!(projection.database_selected());
    assert_eq!(
        locator_files(&path),
        first_files,
        "exact replay must not rotate the generation or rewrite the locator bytes"
    );
}

#[test]
fn concurrent_exact_replay_serializes_without_rotating_the_locator() {
    let (_dir, path) = state_root();
    let arbiter = Arc::new(WorkflowArbiter::new(
        LifecycleStore::open_or_create(&path).unwrap(),
    ));
    let catalog = Arc::new(sqlite_catalog());
    let context = Arc::new(sqlite_context(&path));

    arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("initial selection must succeed");
    let first_files = locator_files(&path);

    let barrier = Arc::new(Barrier::new(4));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let arbiter = Arc::clone(&arbiter);
            let catalog = Arc::clone(&catalog);
            let context = Arc::clone(&context);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                arbiter
                    .select_database(&catalog, &context, &sqlite_backend(), vec![])
                    .map(|(_database, projection)| projection)
            })
        })
        .collect();

    for handle in handles {
        let projection = handle
            .join()
            .expect("replay thread must not panic")
            .expect("every concurrent exact replay must succeed");
        assert!(projection.database_selected());
    }
    assert_eq!(
        locator_files(&path),
        first_files,
        "concurrent exact replay must leave the locator unrotated"
    );
}

#[test]
fn selection_contending_with_a_workflow_serializes_rather_than_failing() {
    let (_dir, path) = state_root();
    let arbiter = Arc::new(WorkflowArbiter::new(
        LifecycleStore::open_or_create(&path).unwrap(),
    ));
    let catalog = Arc::new(sqlite_catalog());
    let context = Arc::new(sqlite_context(&path));
    arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("initial selection must succeed");

    let barrier = Arc::new(Barrier::new(2));
    let selection = {
        let arbiter = Arc::clone(&arbiter);
        let catalog = Arc::clone(&catalog);
        let context = Arc::clone(&context);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            arbiter
                .select_database(&catalog, &context, &sqlite_backend(), vec![])
                .map(|(_database, projection)| projection)
        })
    };
    let workflow = {
        let arbiter = Arc::clone(&arbiter);
        let catalog = Arc::clone(&catalog);
        let context = Arc::clone(&context);
        let barrier = Arc::clone(&barrier);
        thread::spawn(move || {
            barrier.wait();
            arbiter.begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        })
    };

    workflow
        .join()
        .expect("workflow thread must not panic")
        .expect("the contending workflow must serialize and succeed");
    match selection.join().expect("selection thread must not panic") {
        Ok(projection) => assert!(projection.database_selected()),
        Err(error) => assert_eq!(
            error.kind(),
            SelectionFailureKind::Conflict,
            "contention must serialize; only a revalidated lifecycle conflict may fail"
        ),
    }
}

#[test]
fn selection_after_the_record_advances_reports_a_conflict_not_unavailability() {
    let (_dir, path) = state_root();
    let (arbiter, catalog, context) = setup(&path);
    arbiter
        .begin_workflow(&catalog, &context, WorkflowKind::Init, init_metadata())
        .unwrap();

    let error = arbiter
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .map(|_| ())
        .expect_err("selection must be rejected once the record advances");

    assert_eq!(error, SelectionError::NotAllowed);
    assert_eq!(error.kind(), SelectionFailureKind::Conflict);
}

#[test]
fn poisoned_arbiter_reports_unavailability_without_panicking() {
    let (_dir, path) = state_root();
    let arbiter = Arc::new(WorkflowArbiter::new(
        LifecycleStore::open_or_create(&path).unwrap(),
    ));

    let poisoner = {
        let arbiter = Arc::clone(&arbiter);
        let path = path.clone();
        thread::spawn(move || {
            let _ = arbiter.select_database(
                &panicking_catalog(),
                &sqlite_context(&path),
                &panicking_backend(),
                vec![],
            );
        })
    };
    assert!(
        poisoner.join().is_err(),
        "the panicking factory must poison the permit"
    );

    let error = arbiter
        .select_database(
            &sqlite_catalog(),
            &sqlite_context(&path),
            &sqlite_backend(),
            vec![],
        )
        .map(|_| ())
        .expect_err("a poisoned permit must fail closed");
    assert_eq!(
        error,
        SelectionError::Lifecycle(LifecycleError::Persistence)
    );
    assert_eq!(error.kind(), SelectionFailureKind::Unavailable);
    assert_eq!(
        arbiter.projection().unwrap_err(),
        LifecycleError::Persistence
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
