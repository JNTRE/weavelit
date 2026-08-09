use std::sync::Mutex;

use weavelit_server_database::{
    ApplicationDatabase, CheckpointMetadata, DatabaseError, DatabaseInspection, WorkflowCheckpoint,
    WorkflowKind,
};

use crate::{
    BackendCatalog, BackendIdentifier, ConnectionFieldInput, DatabaseLocator, DeploymentRecord,
    LifecycleError, LifecycleProjection, LifecycleState, SelectionError, TrustedBackendContext,
    WorkflowError,
    persistence::{LifecycleStore, RecordPersistencePermit},
};

/// A poisoned permit means a prior mutation panicked, so durable lifecycle state
/// cannot be trusted to have completed safely.
const POISONED: LifecycleError = LifecycleError::Persistence;

/// In-process lifecycle mutation authority that serializes Init and Restore
/// checkpoint ownership and record advancement.
pub struct WorkflowArbiter {
    store: Mutex<LifecycleStore>,
}

impl WorkflowArbiter {
    /// Creates the arbiter, taking exclusive ownership of the lifecycle store.
    pub fn new(store: LifecycleStore) -> Self {
        Self {
            store: Mutex::new(store),
        }
    }

    /// Returns the current deployment record state under the exclusive permit.
    pub fn record_state(&self) -> LifecycleState {
        self.store
            .lock()
            .expect("lifecycle mutex is not poisoned")
            .record()
            .state()
    }

    /// Returns the live lifecycle projection observed under the exclusive permit.
    pub fn projection(&self) -> Result<LifecycleProjection, LifecycleError> {
        let store = self.store.lock().map_err(|_| POISONED)?;
        Ok(project(&store))
    }

    /// Selects, replaces, or replays the Application Database selection.
    ///
    /// Acquires the exclusive permit, rechecks lifecycle eligibility under it, delegates the
    /// durable work to the store, and returns the opened database with the projection observed
    /// under the same permit. An exact replay of the persisted selection changes nothing.
    pub fn select_database(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
        backend: &BackendIdentifier,
        inputs: Vec<ConnectionFieldInput>,
    ) -> Result<(Box<dyn ApplicationDatabase>, LifecycleProjection), SelectionError> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| SelectionError::Lifecycle(POISONED))?;

        if store.record().state() != LifecycleState::Uninitialized {
            return Err(SelectionError::NotAllowed);
        }
        let database = store.select_database(catalog, context, backend, inputs)?;

        Ok((database, project(&store)))
    }

    /// Begins a workflow from eligible selected uninitialized state.
    ///
    /// Acquires the exclusive permit, revalidates current durable state, creates
    /// exactly one deployment-bound checkpoint, then advances the record to
    /// `InitializationPending` in the documented cross-store order.
    pub fn begin_workflow(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
        kind: WorkflowKind,
        metadata: CheckpointMetadata,
    ) -> Result<(), WorkflowError> {
        let mut store = self.store.lock().expect("lifecycle mutex is not poisoned");

        if store.record().state() != LifecycleState::Uninitialized {
            return Err(WorkflowError::NotAllowed);
        }
        let locator = store.locator().ok_or(WorkflowError::DatabaseNotSelected)?;

        let mut db = catalog
            .reopen(locator.settings(), context)
            .map_err(WorkflowError::Lifecycle)?;
        match db
            .inspect(store.record().deployment_identifier())
            .map_err(map_database_error)?
        {
            DatabaseInspection::Uninitialized => {}
            DatabaseInspection::Pending(_) => return Err(WorkflowError::AlreadyPending),
            DatabaseInspection::Initialized { .. } => {
                return Err(WorkflowError::AlreadyInitialized);
            }
        }

        let checkpoint =
            WorkflowCheckpoint::new(store.record().deployment_identifier(), kind, metadata);
        db.create_checkpoint(&checkpoint)
            .map_err(map_database_checkpoint_error)?;

        // A crash here leaves a durable checkpoint that startup classifies as interrupted.
        let generation = store
            .locator()
            .map(DatabaseLocator::generation)
            .ok_or(WorkflowError::DatabaseNotSelected)?;
        let new_record = DeploymentRecord::new(
            store.record().deployment_identifier(),
            LifecycleState::InitializationPending,
            Some(generation),
        )
        .map_err(|_| WorkflowError::Lifecycle(LifecycleError::InvalidState))?;
        let permit = RecordPersistencePermit;
        store
            .replace_record(&permit, new_record)
            .map_err(WorkflowError::Lifecycle)?;

        Ok(())
    }
}

fn project(store: &LifecycleStore) -> LifecycleProjection {
    LifecycleProjection::new(store.locator().is_some())
}

fn map_database_error(error: DatabaseError) -> WorkflowError {
    WorkflowError::Lifecycle(match error {
        DatabaseError::DeploymentMismatch => LifecycleError::DeploymentMismatch,
        DatabaseError::Unavailable => LifecycleError::DependencyUnavailable,
        DatabaseError::IntegrityFailure => LifecycleError::IntegrityFailure,
        DatabaseError::ConfigurationInvalid => LifecycleError::ConfigurationInvalid,
        _ => LifecycleError::InvalidState,
    })
}

fn map_database_checkpoint_error(error: DatabaseError) -> WorkflowError {
    match error {
        DatabaseError::InvalidState => WorkflowError::AlreadyPending,
        DatabaseError::AlreadyInitialized => WorkflowError::AlreadyInitialized,
        _ => map_database_error(error),
    }
}
