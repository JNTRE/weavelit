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
/// checkpoint ownership, record advancement, crash reconciliation, and reset.
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

        // Advance record. A crash here leaves a durable checkpoint; reconcile_workflow
        // or classify_startup (Uninitialized + checkpoint) will complete the transition.
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

    /// Reconciles a crash after checkpoint commit but before record advancement.
    ///
    /// Verifies that the matching durable checkpoint still exists, then advances
    /// the record to `InitializationPending` if not already done. Returns success
    /// when the record is already `InitializationPending` with the matching checkpoint.
    pub fn reconcile_workflow(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
        kind: WorkflowKind,
        metadata: &CheckpointMetadata,
    ) -> Result<(), WorkflowError> {
        let mut store = self.store.lock().expect("lifecycle mutex is not poisoned");

        if !matches!(
            store.record().state(),
            LifecycleState::Uninitialized | LifecycleState::InitializationPending
        ) {
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
            DatabaseInspection::Pending(checkpoint) => {
                validate_checkpoint_match(&store, &checkpoint, kind, metadata)?;
                if store.record().state() == LifecycleState::Uninitialized {
                    advance_to_pending(&mut store)?;
                }
                Ok(())
            }
            DatabaseInspection::Uninitialized => Err(WorkflowError::StateMismatch),
            DatabaseInspection::Initialized { .. } => Err(WorkflowError::AlreadyInitialized),
        }
    }

    /// Resets the matching pending workflow to eligible uninitialized state.
    ///
    /// Uses record-first/checkpoint-second ordering: advances the record to
    /// `Uninitialized` first, then discards the matching pending checkpoint.
    /// A crash after record reset but before checkpoint discard is reconciled
    /// by classify_startup (Uninitialized + checkpoint → InitializationPending)
    /// and requires a retry.
    pub fn reset_workflow(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
        kind: WorkflowKind,
        metadata: &CheckpointMetadata,
    ) -> Result<(), WorkflowError> {
        let mut store = self.store.lock().expect("lifecycle mutex is not poisoned");

        if store.record().state() != LifecycleState::InitializationPending {
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
            DatabaseInspection::Pending(checkpoint) => {
                validate_checkpoint_match(&store, &checkpoint, kind, metadata)?;

                // Record-first: reset to Uninitialized.
                let generation = store
                    .locator()
                    .map(DatabaseLocator::generation)
                    .ok_or(WorkflowError::DatabaseNotSelected)?;
                let uninitialized_record = DeploymentRecord::new(
                    store.record().deployment_identifier(),
                    LifecycleState::Uninitialized,
                    Some(generation),
                )
                .map_err(|_| WorkflowError::Lifecycle(LifecycleError::InvalidState))?;
                let permit = RecordPersistencePermit;
                store
                    .replace_record(&permit, uninitialized_record)
                    .map_err(WorkflowError::Lifecycle)?;

                // Checkpoint-second: discard the matching checkpoint.
                db.discard_checkpoint(store.record().deployment_identifier(), kind)
                    .map_err(map_database_error)?;

                Ok(())
            }
            DatabaseInspection::Uninitialized => Err(WorkflowError::StateMismatch),
            DatabaseInspection::Initialized { .. } => Err(WorkflowError::AlreadyInitialized),
        }
    }
}

fn validate_checkpoint_match(
    store: &LifecycleStore,
    checkpoint: &WorkflowCheckpoint,
    expected_kind: WorkflowKind,
    expected_metadata: &CheckpointMetadata,
) -> Result<(), WorkflowError> {
    if checkpoint.deployment_identifier() != store.record().deployment_identifier() {
        return Err(WorkflowError::Lifecycle(LifecycleError::DeploymentMismatch));
    }
    if checkpoint.workflow() != expected_kind {
        return Err(WorkflowError::StateMismatch);
    }
    if checkpoint.metadata().as_bytes() != expected_metadata.as_bytes() {
        return Err(WorkflowError::StateMismatch);
    }
    Ok(())
}

fn advance_to_pending(store: &mut LifecycleStore) -> Result<(), WorkflowError> {
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
        .map_err(WorkflowError::Lifecycle)
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
