use std::sync::Mutex;

use weavelit_server_database::{
    CheckpointMetadata, DatabaseError, DatabaseInspection, WorkflowCheckpoint, WorkflowKind,
};

use crate::{
    BackendCatalog, DatabaseLocator, DeploymentRecord, LifecycleError, LifecycleState,
    TrustedBackendContext, WorkflowError,
    persistence::{LifecycleStore, RecordPersistencePermit},
};

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
