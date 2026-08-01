#![forbid(unsafe_code)]

//! Restricted lifecycle startup composition for the Weavelit Server.

use std::{
    env,
    path::{Path, PathBuf},
};

use weavelit_server_database_sqlite::SqliteDatabase;
use weavelit_server_lifecycle::{
    ApplicationDatabase, ApplicationDatabaseFactory, BackendCatalog, BackendRegistration,
    LifecycleClassification, LifecycleError, LifecycleStore, TrustedBackendContext,
    ValidatedConnectionSettings, WorkflowKind,
};

const STATE_ROOT_ENV: &str = "WEAVELIT_STATE_ROOT";
const APPLICATION_DATABASE_FILE: &str = "application.sqlite3";

// ---------------------------------------------------------------------------
// SQLite backend factory
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
}

/// Builds the compiled-in SQLite backend catalog.
pub fn sqlite_catalog() -> BackendCatalog {
    BackendCatalog::new(vec![BackendRegistration::new(
        "sqlite",
        vec![],
        Box::new(SqliteFactory),
    )])
    .expect("compiled-in SQLite catalog must be valid")
}

// ---------------------------------------------------------------------------
// Startup classification outcome
// ---------------------------------------------------------------------------

/// Successful restricted startup classification result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupOutcome {
    /// No database has been selected; Server is in restricted pre-operational state.
    UninitializedWithoutDatabase,
    /// A database has been selected; Server is in restricted pre-operational state.
    UninitializedWithDatabase,
    /// Init checkpoint is pending; Server exposes only Init reconciliation.
    InitializationPending(WorkflowKind),
}

// ---------------------------------------------------------------------------
// Startup error
// ---------------------------------------------------------------------------

/// Stable failure category returned by restricted startup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartupError {
    /// The WEAVELIT_STATE_ROOT environment variable is not set.
    StateRootNotConfigured,
    /// The configured state root path is invalid.
    StateRootPathInvalid,
    /// The state root is currently in use by another Server instance.
    StateRootInUse,
    /// Lifecycle persistence could not complete.
    StorageOperationFailed,
    /// The Application Database is unavailable.
    DatabaseUnavailable,
    /// Anchor set is missing, malformed, or cannot be trusted.
    AnchorSetInvalid,
    /// Anchor format version is not supported.
    AnchorVersionUnsupported,
    /// Anchor binding does not match this deployment.
    AnchorBindingInvalid,
    /// Database integrity or schema cannot be trusted.
    DatabaseIntegrityFailure,
    /// Startup state combination is invalid or unsupported.
    StateCombinationInvalid,
}

impl StartupError {
    /// Returns the stable category/reason pair for centralized error presentation.
    pub fn category_reason(&self) -> (&'static str, &'static str) {
        match self {
            Self::StateRootNotConfigured => ("configuration_invalid", "state_root_not_configured"),
            Self::StateRootPathInvalid => ("configuration_invalid", "state_root_path_invalid"),
            Self::StateRootInUse => ("preoperational_unavailable", "state_root_in_use"),
            Self::StorageOperationFailed => ("storage_unavailable", "storage_operation_failed"),
            Self::DatabaseUnavailable => ("storage_unavailable", "database_unavailable"),
            Self::AnchorSetInvalid => ("storage_integrity_failure", "anchor_set_invalid"),
            Self::AnchorVersionUnsupported => {
                ("storage_integrity_failure", "anchor_version_unsupported")
            }
            Self::AnchorBindingInvalid => ("storage_integrity_failure", "anchor_binding_invalid"),
            Self::DatabaseIntegrityFailure => {
                ("storage_integrity_failure", "database_integrity_failure")
            }
            Self::StateCombinationInvalid => {
                ("deployment_state_invalid", "state_combination_invalid")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Startup composition
// ---------------------------------------------------------------------------

/// Reads the trusted state root from host/process configuration.
///
/// Returns `Err(StartupError::StateRootNotConfigured)` if the environment
/// variable is absent, or `Err(StartupError::StateRootPathInvalid)` if the
/// value is empty or not an absolute path string.
pub fn read_state_root() -> Result<PathBuf, StartupError> {
    let value = env::var(STATE_ROOT_ENV).map_err(|_| StartupError::StateRootNotConfigured)?;
    if value.is_empty() {
        return Err(StartupError::StateRootPathInvalid);
    }
    let path = PathBuf::from(&value);
    if !path.is_absolute() {
        return Err(StartupError::StateRootPathInvalid);
    }
    Ok(path)
}

/// Maps a lifecycle open/creation error to a startup error category.
fn map_open_error(error: LifecycleError) -> StartupError {
    match error {
        LifecycleError::ConfigurationInvalid => StartupError::StateRootPathInvalid,
        LifecycleError::DependencyUnavailable => StartupError::StateRootInUse,
        LifecycleError::Persistence => StartupError::StorageOperationFailed,
        LifecycleError::IntegrityFailure => StartupError::AnchorSetInvalid,
        LifecycleError::UnsupportedVersion => StartupError::AnchorVersionUnsupported,
        LifecycleError::DeploymentMismatch => StartupError::AnchorBindingInvalid,
        LifecycleError::InvalidState => StartupError::AnchorSetInvalid,
        _ => StartupError::StorageOperationFailed,
    }
}

/// Maps a lifecycle classification error to a startup error category.
fn map_classification_error(error: LifecycleError) -> StartupError {
    match error {
        LifecycleError::DependencyUnavailable => StartupError::DatabaseUnavailable,
        LifecycleError::IntegrityFailure => StartupError::DatabaseIntegrityFailure,
        LifecycleError::DeploymentMismatch => StartupError::AnchorBindingInvalid,
        LifecycleError::ConfigurationInvalid => StartupError::StateRootPathInvalid,
        LifecycleError::InvalidState => StartupError::StateCombinationInvalid,
        LifecycleError::UnsupportedVersion => StartupError::AnchorVersionUnsupported,
        LifecycleError::Persistence => StartupError::StorageOperationFailed,
        _ => StartupError::StorageOperationFailed,
    }
}

/// Composes the lifecycle crate and SQLite backend, opens or creates the anchor
/// set, and classifies startup state.
///
/// Returns a `StartupOutcome` for every supported restricted state.
/// Fails closed for `Initialized` and `PostCommitReconciliationRequired` states
/// since normal operation and sealing are not yet implemented.
pub fn classify_restricted_startup(state_root: &Path) -> Result<StartupOutcome, StartupError> {
    let mut store = LifecycleStore::open_or_create(state_root).map_err(map_open_error)?;

    let catalog = sqlite_catalog();
    let context = TrustedBackendContext::new(state_root.join(APPLICATION_DATABASE_FILE));

    let classification = store
        .classify_startup(&catalog, &context)
        .map_err(map_classification_error)?;

    match classification {
        LifecycleClassification::UninitializedWithoutDatabase => {
            Ok(StartupOutcome::UninitializedWithoutDatabase)
        }
        LifecycleClassification::UninitializedWithDatabase => {
            Ok(StartupOutcome::UninitializedWithDatabase)
        }
        LifecycleClassification::InitializationPending(kind) => {
            Ok(StartupOutcome::InitializationPending(kind))
        }
        // Fail closed for states not yet handled by this milestone.
        LifecycleClassification::PostCommitReconciliationRequired
        | LifecycleClassification::Initialized => Err(StartupError::StateCombinationInvalid),
    }
}
