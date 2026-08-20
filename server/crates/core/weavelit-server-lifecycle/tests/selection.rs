use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use weavelit_server_database::{
    ApplicationState, CheckpointMetadata, DatabaseError, DatabaseInspection, DeploymentIdentifier,
    InitializedState, StateIdentifier,
};
use weavelit_server_database_sqlite::{RetainedSqliteInspection, SqliteDatabase};
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendIdentifier,
    BackendOpenError, BackendRegistration, ConnectionFieldDeclaration, ConnectionFieldIdentifier,
    ConnectionFieldInput, ConnectionFieldRequirement, ConnectionValidationError, ConnectionValue,
    ConnectionValueKind, FieldDeclarationError, LifecycleError, LifecycleStore,
    RetainedDatabaseInspection, SecretClassification, SelectedDatabase, SelectionError,
    SelectionFailureKind, TrustedBackendContext, ValidatedConnectionSettings, WorkflowCheckpoint,
    WorkflowKind,
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

fn expect_selection_error(result: Result<SelectedDatabase, SelectionError>) -> SelectionError {
    match result {
        Ok(_) => panic!("selection unexpectedly succeeded"),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// Real SQLite factory and helpers
// ---------------------------------------------------------------------------

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
        expected_deployment_identifier: DeploymentIdentifier,
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

// ---------------------------------------------------------------------------
// Fake factory helpers
// ---------------------------------------------------------------------------

struct FakeDatabase {
    inspection: DatabaseInspection,
}

impl ApplicationDatabase for FakeDatabase {
    fn inspect(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        Ok(self.inspection.clone())
    }

    fn create_checkpoint(&mut self, _checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError> {
        Ok(())
    }

    fn complete_checkpoint(
        &mut self,
        _checkpoint: &WorkflowCheckpoint,
        _state: &ApplicationState,
        _reconciliation: &weavelit_server_database::ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::InvalidState)
    }

    fn load_initialized_state(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn acknowledge_completion(
        &mut self,
        _expected_deployment_identifier: DeploymentIdentifier,
        _record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_human_authorization(
        &mut self,
        _account: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::HumanAuthorizationSnapshot>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_account_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _account: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::AccountAuditReference>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_group_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _group: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::GroupAuditReference>, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn load_log_configuration_audit_reference(
        &mut self,
        _persistence: &weavelit_server_database::AuditReferencePersistence,
        _configuration: StateIdentifier,
    ) -> Result<Option<weavelit_server_database::LogConfigurationAuditReference>, DatabaseError>
    {
        Err(DatabaseError::NotInitialized)
    }

    fn load_component_enablement(
        &mut self,
    ) -> Result<weavelit_server_database::ComponentEnablement, DatabaseError> {
        Err(DatabaseError::NotInitialized)
    }

    fn sessions(&mut self) -> Option<&mut dyn weavelit_server_database::SessionStore> {
        None
    }

    fn mfa(&mut self) -> Option<&mut dyn weavelit_server_database::MfaStore> {
        None
    }

    fn reconciliation(&mut self) -> Option<&mut dyn weavelit_server_database::ReconciliationStore> {
        None
    }

    fn audit_terminal_recovery(
        &mut self,
    ) -> Option<&mut dyn weavelit_server_database::AuditTerminalRecoveryStore> {
        None
    }

    fn close(self: Box<Self>) -> Result<(), DatabaseError> {
        Ok(())
    }
}

struct FakeFactory {
    result: Result<DatabaseInspection, LifecycleError>,
}

impl ApplicationDatabaseFactory for FakeFactory {
    fn open(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        match &self.result {
            Ok(inspection) => Ok(Box::new(FakeDatabase {
                inspection: inspection.clone(),
            })),
            Err(e) => Err(*e),
        }
    }

    fn inspect_retained(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        self.result
            .clone()
            .map(RetainedDatabaseInspection::Inspected)
    }
}

fn fake_catalog(inspection: Result<DatabaseInspection, LifecycleError>) -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "fake-backend",
        vec![],
        Box::new(FakeFactory { result: inspection }),
    )])
    .unwrap()
}

fn fake_backend() -> BackendIdentifier {
    BackendIdentifier::new("fake-backend").unwrap()
}

fn fake_context() -> TrustedBackendContext {
    TrustedBackendContext::new(PathBuf::from("/fake/path/application.sqlite3"))
}

fn secret_field_backend() -> BackendIdentifier {
    BackendIdentifier::new("secret-field-backend").unwrap()
}

fn secret_field_catalog() -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "secret-field-backend",
        vec![
            ConnectionFieldDeclaration::new(
                "credential",
                ConnectionValueKind::String,
                ConnectionFieldRequirement::Optional,
                SecretClassification::Secret,
            )
            .unwrap(),
        ],
        Box::new(FakeFactory {
            result: Ok(DatabaseInspection::Uninitialized),
        }),
    )])
    .unwrap()
}

fn credential_input(value: &str) -> ConnectionFieldInput {
    ConnectionFieldInput::new(
        ConnectionFieldIdentifier::new("credential").unwrap(),
        SecretClassification::Secret,
        ConnectionValue::string(value),
    )
}

fn fake_checkpoint(deployment_identifier: DeploymentIdentifier) -> WorkflowCheckpoint {
    WorkflowCheckpoint::new(
        deployment_identifier,
        WorkflowKind::Init,
        CheckpointMetadata::from_bytes(b"meta".as_slice()).unwrap(),
    )
}

// Controllable fake factory for replacement-scenario tests.
struct ControllableFactory {
    result: Arc<Mutex<Result<DatabaseInspection, LifecycleError>>>,
}

impl ApplicationDatabaseFactory for ControllableFactory {
    fn open(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
    ) -> Result<Box<dyn ApplicationDatabase>, LifecycleError> {
        let guard = self.result.lock().unwrap();
        match guard.clone() {
            Ok(inspection) => Ok(Box::new(FakeDatabase { inspection })),
            Err(e) => Err(e),
        }
    }

    fn inspect_retained(
        &self,
        _context: &TrustedBackendContext,
        _settings: &ValidatedConnectionSettings,
        _expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<RetainedDatabaseInspection, LifecycleError> {
        self.result
            .lock()
            .unwrap()
            .clone()
            .map(RetainedDatabaseInspection::Inspected)
    }
}

fn controllable_catalog(
    result: Arc<Mutex<Result<DatabaseInspection, LifecycleError>>>,
) -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "fake-backend",
        vec![],
        Box::new(ControllableFactory { result }),
    )])
    .unwrap()
}

// ---------------------------------------------------------------------------
// Tests: connection-field validation rejections
// ---------------------------------------------------------------------------

#[test]
fn unknown_backend_is_rejected_before_locator_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = fake_catalog(Ok(DatabaseInspection::Uninitialized));
    let unknown = BackendIdentifier::new("unknown-backend").unwrap();

    let error =
        expect_selection_error(store.select_database(&catalog, &fake_context(), &unknown, vec![]));

    assert_eq!(
        error,
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::UnknownBackend
        ))
    );
    assert!(store.locator().is_none());
}

#[test]
fn missing_required_field_rejected_before_locator_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = BackendCatalog::new(vec![BackendRegistration::new(
        "requires-field",
        vec![
            ConnectionFieldDeclaration::new(
                "token",
                ConnectionValueKind::String,
                ConnectionFieldRequirement::Required,
                SecretClassification::Secret,
            )
            .unwrap(),
        ],
        Box::new(FakeFactory {
            result: Ok(DatabaseInspection::Uninitialized),
        }),
    )])
    .unwrap();
    let backend = BackendIdentifier::new("requires-field").unwrap();

    let error =
        expect_selection_error(store.select_database(&catalog, &fake_context(), &backend, vec![]));

    assert_eq!(
        error,
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::MissingRequiredField
        ))
    );
    assert!(store.locator().is_none());
}

#[test]
fn duplicate_field_rejected_before_locator_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = BackendCatalog::new(vec![BackendRegistration::new(
        "backend-with-field",
        vec![
            ConnectionFieldDeclaration::new(
                "count",
                ConnectionValueKind::Integer,
                ConnectionFieldRequirement::Optional,
                SecretClassification::NonSecret,
            )
            .unwrap(),
        ],
        Box::new(FakeFactory {
            result: Ok(DatabaseInspection::Uninitialized),
        }),
    )])
    .unwrap();
    let backend = BackendIdentifier::new("backend-with-field").unwrap();
    let count_id = ConnectionFieldIdentifier::new("count").unwrap();

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &backend,
        vec![
            ConnectionFieldInput::new(
                count_id.clone(),
                SecretClassification::NonSecret,
                ConnectionValue::integer(1),
            ),
            ConnectionFieldInput::new(
                count_id,
                SecretClassification::NonSecret,
                ConnectionValue::integer(2),
            ),
        ],
    ));

    assert_eq!(
        error,
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::DuplicateField
        ))
    );
    assert!(store.locator().is_none());
}

#[test]
fn wrong_value_kind_rejected_before_locator_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = BackendCatalog::new(vec![BackendRegistration::new(
        "backend-with-field",
        vec![
            ConnectionFieldDeclaration::new(
                "count",
                ConnectionValueKind::Integer,
                ConnectionFieldRequirement::Optional,
                SecretClassification::NonSecret,
            )
            .unwrap(),
        ],
        Box::new(FakeFactory {
            result: Ok(DatabaseInspection::Uninitialized),
        }),
    )])
    .unwrap();
    let backend = BackendIdentifier::new("backend-with-field").unwrap();
    let count_id = ConnectionFieldIdentifier::new("count").unwrap();

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &backend,
        vec![ConnectionFieldInput::new(
            count_id,
            SecretClassification::NonSecret,
            ConnectionValue::string("not-an-integer"),
        )],
    ));

    assert_eq!(
        error,
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::WrongValueKind
        ))
    );
}

#[test]
fn path_and_file_reference_field_declarations_are_forbidden() {
    for identifier in [
        "path",
        "database-path",
        "credential-file",
        "config-filename",
        "storage-directory",
        "cache-dir",
    ] {
        assert_eq!(
            ConnectionFieldDeclaration::new(
                identifier,
                ConnectionValueKind::String,
                ConnectionFieldRequirement::Optional,
                SecretClassification::NonSecret,
            )
            .unwrap_err(),
            FieldDeclarationError::LocalReferenceForbidden,
            "identifier '{identifier}' must be rejected"
        );
    }
}

// ---------------------------------------------------------------------------
// Tests: candidate eligibility rejections
// ---------------------------------------------------------------------------

#[test]
fn pending_candidate_database_is_rejected() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let checkpoint = fake_checkpoint(store.record().deployment_identifier());
    let catalog = fake_catalog(Ok(DatabaseInspection::Pending(checkpoint)));

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &fake_backend(),
        vec![],
    ));

    assert_eq!(error, SelectionError::CandidateIneligible);
    assert!(store.locator().is_none());
}

#[test]
fn initialized_candidate_database_is_rejected() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let deployment_identifier = store.record().deployment_identifier();
    let catalog = fake_catalog(Ok(DatabaseInspection::Initialized {
        deployment_identifier,
    }));

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &fake_backend(),
        vec![],
    ));

    assert_eq!(error, SelectionError::CandidateIneligible);
    assert!(store.locator().is_none());
}

#[test]
fn factory_failure_is_rejected_before_locator_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = fake_catalog(Err(LifecycleError::DependencyUnavailable));

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &fake_backend(),
        vec![],
    ));

    assert_eq!(
        error,
        SelectionError::Open(BackendOpenError::Factory(
            LifecycleError::DependencyUnavailable
        ))
    );
    assert!(store.locator().is_none());
}

// ---------------------------------------------------------------------------
// Tests: successful initial selection with real SQLite
// ---------------------------------------------------------------------------

#[test]
fn initial_selection_persists_locator_and_returns_database() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();

    store
        .select_database(
            &sqlite_catalog(),
            &sqlite_context(&path),
            &sqlite_backend(),
            vec![],
        )
        .expect("initial selection must succeed");

    assert!(store.locator().is_some());
    assert_eq!(
        store.locator().unwrap().backend_identifier().as_str(),
        SQLITE_BACKEND
    );
    assert!(path.join("application.sqlite3").exists());
}

#[test]
fn selected_locator_survives_restart() {
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
    assert!(store.locator().is_some(), "locator must survive restart");
    assert_eq!(
        store.locator().unwrap().backend_identifier().as_str(),
        SQLITE_BACKEND
    );
}

// ---------------------------------------------------------------------------
// Tests: SQLite path isolation
// ---------------------------------------------------------------------------

#[test]
fn sqlite_database_path_is_derived_from_state_root_not_client() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();

    store
        .select_database(
            &sqlite_catalog(),
            &sqlite_context(&path),
            &sqlite_backend(),
            vec![],
        )
        .unwrap();

    let db_path = path.join("application.sqlite3");
    assert!(db_path.exists());
    assert_eq!(fs::metadata(&db_path).unwrap().mode() & 0o777, 0o600);
    assert!(
        store.locator().unwrap().settings().is_empty(),
        "SQLite locator must have no persisted client fields"
    );
}

// ---------------------------------------------------------------------------
// Tests: replacement and exact replay
// ---------------------------------------------------------------------------

#[test]
fn replacement_with_different_settings_rotates_the_locator() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = secret_field_catalog();
    let backend = secret_field_backend();

    store
        .select_database(
            &catalog,
            &fake_context(),
            &backend,
            vec![credential_input("first-credential")],
        )
        .expect("initial selection must succeed");
    let first_generation = store.locator().unwrap().generation();
    let first_files = locator_files(&path);

    store
        .select_database(
            &catalog,
            &fake_context(),
            &backend,
            vec![credential_input("second-credential")],
        )
        .expect("replacement must succeed");

    assert_ne!(store.locator().unwrap().generation(), first_generation);
    assert_ne!(locator_files(&path), first_files);
}

#[test]
fn exact_replay_leaves_the_locator_generation_and_bytes_unchanged() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);

    store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("initial selection must succeed");
    let first_generation = store.locator().unwrap().generation();
    let first_files = locator_files(&path);

    store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .expect("exact replay must succeed");

    assert_eq!(
        store.locator().unwrap().generation(),
        first_generation,
        "exact replay must not rotate the locator generation"
    );
    assert_eq!(
        locator_files(&path),
        first_files,
        "exact replay must not rewrite the locator bytes"
    );
}

#[test]
fn exact_replay_of_secret_settings_leaves_the_locator_unchanged() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = secret_field_catalog();
    let backend = secret_field_backend();

    store
        .select_database(
            &catalog,
            &fake_context(),
            &backend,
            vec![credential_input("replayed-credential")],
        )
        .expect("initial selection must succeed");
    let first_generation = store.locator().unwrap().generation();
    let first_files = locator_files(&path);

    store
        .select_database(
            &catalog,
            &fake_context(),
            &backend,
            vec![credential_input("replayed-credential")],
        )
        .expect("exact replay must succeed");

    assert_eq!(store.locator().unwrap().generation(), first_generation);
    assert_eq!(locator_files(&path), first_files);
}

#[test]
fn replacement_rejected_when_current_database_has_checkpoint() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    let catalog = sqlite_catalog();
    let context = sqlite_context(&path);

    let mut db = store
        .select_database(&catalog, &context, &sqlite_backend(), vec![])
        .unwrap();

    let checkpoint = fake_checkpoint(store.record().deployment_identifier());
    db.with(|database| database.create_checkpoint(&checkpoint))
        .unwrap();
    drop(db);

    let error = expect_selection_error(store.select_database(
        &catalog,
        &context,
        &sqlite_backend(),
        vec![],
    ));

    assert_eq!(error, SelectionError::ReplacementIneligible);
    assert!(
        store.locator().is_some(),
        "original locator must be preserved"
    );
}

#[test]
fn replacement_rejected_after_initialized_state() {
    let (_dir, path) = state_root();
    let result = Arc::new(Mutex::new(Ok(DatabaseInspection::Uninitialized)));
    let catalog = controllable_catalog(Arc::clone(&result));

    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    store
        .select_database(&catalog, &fake_context(), &fake_backend(), vec![])
        .unwrap();

    let deployment_identifier = store.record().deployment_identifier();
    *result.lock().unwrap() = Ok(DatabaseInspection::Initialized {
        deployment_identifier,
    });

    let error = expect_selection_error(store.select_database(
        &catalog,
        &fake_context(),
        &fake_backend(),
        vec![],
    ));

    assert_eq!(error, SelectionError::ReplacementIneligible);
}

// ---------------------------------------------------------------------------
// Tests: restart reopening
// ---------------------------------------------------------------------------

#[test]
fn reopen_selected_database_returns_correct_backend_after_restart() {
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
    let mut db = store
        .reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path))
        .expect("reopening must succeed");

    assert_eq!(
        db.with(|database| database.inspect(store.record().deployment_identifier()))
            .unwrap(),
        DatabaseInspection::Uninitialized
    );
}

#[test]
fn reopen_without_locator_fails_closed() {
    let (_dir, path) = state_root();
    let store = LifecycleStore::open_or_create(&path).unwrap();

    let error = match store.reopen_selected_database(&sqlite_catalog(), &sqlite_context(&path)) {
        Ok(_) => panic!("reopen must fail without locator"),
        Err(e) => e,
    };
    assert_eq!(error, LifecycleError::InvalidState);
}

// ---------------------------------------------------------------------------
// Tests: crash-point safety
// ---------------------------------------------------------------------------

#[test]
fn orphan_locator_from_crash_before_record_commit_fails_closed_without_mutation() {
    let (_dir, path) = state_root();
    let mut store = LifecycleStore::open_or_create(&path).unwrap();
    store
        .select_database(
            &sqlite_catalog(),
            &sqlite_context(&path),
            &sqlite_backend(),
            vec![],
        )
        .unwrap();
    drop(store);

    let orphan_bytes = [0xAB_u8; 16];
    let orphan_name = format!(
        "database-locator-{}.json",
        base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            orphan_bytes
        )
    );
    let orphan_path = path.join(&orphan_name);
    let orphan_contents = b"orphan-placeholder";
    fs::write(&orphan_path, orphan_contents).unwrap();

    assert_eq!(
        LifecycleStore::open_or_create(&path).unwrap_err(),
        LifecycleError::IntegrityFailure
    );
    assert_eq!(fs::read(orphan_path).unwrap(), orphan_contents);
}

// ---------------------------------------------------------------------------
// Tests: failure families
// ---------------------------------------------------------------------------

#[test]
fn selection_failures_separate_the_conflict_and_unavailable_families() {
    for conflict in [
        SelectionError::NotAllowed,
        SelectionError::CandidateIneligible,
        SelectionError::ReplacementIneligible,
    ] {
        assert_eq!(
            conflict.kind(),
            SelectionFailureKind::Conflict,
            "{conflict:?} must be a conflict"
        );
    }
    for unavailable in [
        SelectionError::Lifecycle(LifecycleError::Persistence),
        SelectionError::Lifecycle(LifecycleError::DependencyUnavailable),
        SelectionError::Lifecycle(LifecycleError::IntegrityFailure),
        SelectionError::Open(BackendOpenError::Factory(
            LifecycleError::DependencyUnavailable,
        )),
    ] {
        assert_eq!(
            unavailable.kind(),
            SelectionFailureKind::Unavailable,
            "{unavailable:?} must be unavailable"
        );
    }
    assert_eq!(
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::UnknownBackend
        ))
        .kind(),
        SelectionFailureKind::RequestInvalid
    );
}

// ---------------------------------------------------------------------------
// Tests: redaction
// ---------------------------------------------------------------------------

#[test]
fn selection_errors_do_not_expose_sensitive_values() {
    let sensitive_path = "/private/secrets/application.sqlite3";
    let sensitive_cred = "secret-credential-value";

    let errors: &[SelectionError] = &[
        SelectionError::Open(BackendOpenError::ConnectionInvalid(
            ConnectionValidationError::UnknownBackend,
        )),
        SelectionError::NotAllowed,
        SelectionError::CandidateIneligible,
        SelectionError::ReplacementIneligible,
        SelectionError::Lifecycle(LifecycleError::DependencyUnavailable),
        SelectionError::Lifecycle(LifecycleError::IntegrityFailure),
        SelectionError::Lifecycle(LifecycleError::DeploymentMismatch),
        SelectionError::Lifecycle(LifecycleError::Persistence),
    ];
    for error in errors {
        let output = format!("{error:?} {error}");
        assert!(
            !output.contains(sensitive_path),
            "error must not expose path: {output}"
        );
        assert!(
            !output.contains(sensitive_cred),
            "error must not expose credential: {output}"
        );
    }
}
