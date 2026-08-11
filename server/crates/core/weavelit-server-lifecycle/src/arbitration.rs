use std::{
    fmt,
    sync::{Mutex, MutexGuard},
};

use weavelit_server_database::{
    ApplicationDatabase, ApplicationState, CheckpointMetadata, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, InitializedState, ProtectedValue, StateIdentifier, WorkflowCheckpoint,
    WorkflowKind,
};
use zeroize::Zeroizing;

use crate::{
    BackendCatalog, BackendIdentifier, ConnectionFieldInput, DatabaseLocator, DeploymentRecord,
    LifecycleError, LifecycleProjection, LifecycleState, ProtectedValueKind, ProtectedValueOpener,
    ProtectedValueSealer, SelectionError, TrustedBackendContext, WorkflowError,
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

    /// Authorizes one exclusive workflow attempt from eligible selected state.
    ///
    /// Acquires the exclusive permit, revalidates current durable state, and
    /// opens the selected database. No durable change occurs yet, so a caller
    /// may still fail without leaving retained partial state. The returned
    /// permit holds the exclusive lock until the workflow ends.
    pub fn authorize_workflow<'arbiter>(
        &'arbiter self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
    ) -> Result<WorkflowPermit<'arbiter>, WorkflowError> {
        let store = self
            .store
            .lock()
            .map_err(|_| WorkflowError::Lifecycle(POISONED))?;

        // A sealed record is reported as its own result rather than folded into
        // the generic refusal, because a workflow that must answer
        // "already initialized" to a direct in-process call cannot infer that
        // from `NotAllowed` without re-reading the record it is not allowed to
        // read. The match is exhaustive so a new lifecycle state fails to
        // compile until this authority states how it answers for it.
        match store.record().state() {
            LifecycleState::Uninitialized => {}
            LifecycleState::InitializationPending => return Err(WorkflowError::NotAllowed),
            LifecycleState::Initialized => return Err(WorkflowError::AlreadyInitialized),
        }
        let locator = store.locator().ok_or(WorkflowError::DatabaseNotSelected)?;

        let mut database = catalog
            .reopen(locator.settings(), context)
            .map_err(WorkflowError::Lifecycle)?;
        match database
            .inspect(store.record().deployment_identifier())
            .map_err(map_database_error)?
        {
            DatabaseInspection::Uninitialized => {}
            DatabaseInspection::Pending(_) => return Err(WorkflowError::AlreadyPending),
            DatabaseInspection::Initialized { .. } => {
                return Err(WorkflowError::AlreadyInitialized);
            }
        }

        Ok(WorkflowPermit { store, database })
    }

    /// Loads a sealed deployment's application state under the exclusive permit.
    ///
    /// Startup classification is a routing control, not the authority, so this
    /// re-reads the deployment record and independently re-inspects the
    /// database exactly as sealing does. A record and database that no longer
    /// agree, or that are bound to another deployment, fail closed rather than
    /// producing a surface to serve.
    pub fn load_sealed_deployment(
        &self,
        catalog: &BackendCatalog,
        context: &TrustedBackendContext,
    ) -> Result<SealedDeployment, WorkflowError> {
        let store = self
            .store
            .lock()
            .map_err(|_| WorkflowError::Lifecycle(POISONED))?;

        if store.record().state() != LifecycleState::Initialized {
            return Err(WorkflowError::NotAllowed);
        }
        let deployment_identifier = store.record().deployment_identifier();
        let locator = store.locator().ok_or(WorkflowError::DatabaseNotSelected)?;

        let mut database = catalog
            .reopen(locator.settings(), context)
            .map_err(WorkflowError::Lifecycle)?;
        match database
            .inspect(deployment_identifier)
            .map_err(map_database_error)?
        {
            DatabaseInspection::Initialized {
                deployment_identifier: initialized,
            } if initialized == deployment_identifier => {}
            _ => return Err(WorkflowError::StateMismatch),
        }

        let state = database
            .load_initialized_state(deployment_identifier)
            .map_err(map_database_error)?;
        if state.deployment_identifier() != deployment_identifier
            || !state.completion_acknowledged()
        {
            return Err(WorkflowError::StateMismatch);
        }

        Ok(SealedDeployment { state, database })
    }
}

/// At-rest protection reached through the same serialized lifecycle authority
/// every durable mutation passes through.
///
/// An operational runtime that seals or opens an enrolled factor holds the
/// arbiter rather than the store, so it reaches the deployment's key through
/// the one lock a Restore also holds while it replaces state. It therefore
/// cannot seal a value against a key generation a running workflow is
/// replacing.
impl ProtectedValueSealer for WorkflowArbiter {
    fn seal(
        &self,
        kind: ProtectedValueKind,
        plaintext: &[u8],
    ) -> Result<ProtectedValue, LifecycleError> {
        self.store
            .lock()
            .map_err(|_| POISONED)?
            .seal(kind, plaintext)
    }
}

impl ProtectedValueOpener for WorkflowArbiter {
    fn open(
        &self,
        kind: ProtectedValueKind,
        value: &ProtectedValue,
    ) -> Result<Zeroizing<Vec<u8>>, LifecycleError> {
        self.store.lock().map_err(|_| POISONED)?.open(kind, value)
    }
}

/// A sealed deployment's loaded application state and its open database.
///
/// The runtime holds this for the process lifetime, so the Application Database
/// opened during startup stays open rather than being reopened per request.
pub struct SealedDeployment {
    state: InitializedState,
    database: Box<dyn ApplicationDatabase>,
}

impl SealedDeployment {
    /// Returns the loaded initialized application state.
    pub const fn state(&self) -> &InitializedState {
        &self.state
    }

    /// Returns the Application Database this deployment holds open.
    pub fn database(&mut self) -> &mut dyn ApplicationDatabase {
        &mut *self.database
    }

    /// Consumes the sealed deployment and hands its loaded state and its open
    /// database to the runtime that will own them.
    ///
    /// The database is moved rather than reopened, so an operational runtime
    /// serves from the handle sealing or startup already opened instead of
    /// creating a second one against the same target.
    #[must_use]
    pub fn into_parts(self) -> (InitializedState, Box<dyn ApplicationDatabase>) {
        (self.state, self.database)
    }
}

impl fmt::Debug for SealedDeployment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("SealedDeployment(REDACTED)")
    }
}

/// Exclusive authority to begin one workflow against the selected database.
///
/// Holding this permit blocks every other lifecycle mutation, so a caller
/// completes its preparation and releases it rather than retaining it.
pub struct WorkflowPermit<'arbiter> {
    store: MutexGuard<'arbiter, LifecycleStore>,
    database: Box<dyn ApplicationDatabase>,
}

impl<'arbiter> WorkflowPermit<'arbiter> {
    /// Returns the deployment identifier every created state must be bound to.
    pub fn deployment_identifier(&self) -> DeploymentIdentifier {
        self.store.record().deployment_identifier()
    }

    /// Returns the Application Database backend this workflow is bound to.
    ///
    /// Authorization already established the locator, so a permit that exists
    /// always has one; this reports it rather than restating that condition as
    /// a failure the caller must handle again.
    pub fn selected_backend(&self) -> &BackendIdentifier {
        self.store
            .locator()
            .map(DatabaseLocator::backend_identifier)
            .expect("authorization established the selected database locator")
    }

    /// Returns the capability that protects application secrets at rest.
    pub fn sealer(&self) -> &dyn ProtectedValueSealer {
        &*self.store
    }

    /// Creates the deployment-bound checkpoint and advances the record.
    ///
    /// This is the workflow's point of no return: it makes the other workflow
    /// permanently unavailable, and any later failure leaves retained partial
    /// state that only a redeploy resolves.
    pub fn create_checkpoint(
        mut self,
        kind: WorkflowKind,
        metadata: CheckpointMetadata,
    ) -> Result<PendingWorkflow<'arbiter>, WorkflowError> {
        let checkpoint =
            WorkflowCheckpoint::new(self.store.record().deployment_identifier(), kind, metadata);
        self.database
            .create_checkpoint(&checkpoint)
            .map_err(map_database_checkpoint_error)?;

        // A crash here leaves a durable checkpoint that startup classifies as interrupted.
        let generation = self
            .store
            .locator()
            .map(DatabaseLocator::generation)
            .ok_or(WorkflowError::DatabaseNotSelected)?;
        let new_record = DeploymentRecord::new(
            self.store.record().deployment_identifier(),
            LifecycleState::InitializationPending,
            Some(generation),
        )
        .map_err(|_| WorkflowError::Lifecycle(LifecycleError::InvalidState))?;
        self.store
            .replace_record(&RecordPersistencePermit, new_record)
            .map_err(WorkflowError::Lifecycle)?;

        Ok(PendingWorkflow {
            store: self.store,
            database: self.database,
            checkpoint,
        })
    }
}

/// A durable checkpoint awaiting its one atomic state replacement.
pub struct PendingWorkflow<'arbiter> {
    store: MutexGuard<'arbiter, LifecycleStore>,
    database: Box<dyn ApplicationDatabase>,
    checkpoint: WorkflowCheckpoint,
}

impl<'arbiter> PendingWorkflow<'arbiter> {
    /// Atomically replaces this exact checkpoint with complete application state.
    pub fn complete_checkpoint(
        mut self,
        state: &ApplicationState,
    ) -> Result<CommittedWorkflow<'arbiter>, WorkflowError> {
        self.database
            .complete_checkpoint(&self.checkpoint, state)
            .map_err(map_database_completion_error)?;

        Ok(CommittedWorkflow {
            store: self.store,
            database: self.database,
        })
    }
}

/// Committed application state whose completion obligation is still outstanding.
pub struct CommittedWorkflow<'arbiter> {
    store: MutexGuard<'arbiter, LifecycleStore>,
    database: Box<dyn ApplicationDatabase>,
}

impl<'arbiter> CommittedWorkflow<'arbiter> {
    /// Marks the persisted completion obligation acknowledged exactly once.
    ///
    /// The caller must have obtained the durable acknowledgement for this record
    /// from the committed System Log assignment before calling this.
    pub fn acknowledge_completion(
        mut self,
        record_identifier: StateIdentifier,
    ) -> Result<AcknowledgedWorkflow<'arbiter>, WorkflowError> {
        let deployment_identifier = self.store.record().deployment_identifier();
        self.database
            .acknowledge_completion(deployment_identifier, record_identifier)
            .map_err(map_database_completion_error)?;

        Ok(AcknowledgedWorkflow {
            store: self.store,
            database: self.database,
        })
    }
}

/// Acknowledged application state eligible to seal the deployment.
pub struct AcknowledgedWorkflow<'arbiter> {
    store: MutexGuard<'arbiter, LifecycleStore>,
    database: Box<dyn ApplicationDatabase>,
}

impl AcknowledgedWorkflow<'_> {
    /// Seals the deployment record `Initialized` and returns the sealed
    /// deployment: its loaded state and the database the workflow held open.
    ///
    /// Every fallible step runs before the record is written, so the record
    /// advances only once the deployment is known to be complete, acknowledged,
    /// and loadable. The workflow's open database is retained rather than
    /// dropped, so an in-process activation continues on the same handle the
    /// workflow committed through instead of reopening the target.
    pub fn seal(mut self) -> Result<SealedDeployment, WorkflowError> {
        let deployment_identifier = self.store.record().deployment_identifier();
        if self.store.record().state() != LifecycleState::InitializationPending {
            return Err(WorkflowError::NotAllowed);
        }
        match self
            .database
            .inspect(deployment_identifier)
            .map_err(map_database_error)?
        {
            DatabaseInspection::Initialized {
                deployment_identifier: initialized,
            } if initialized == deployment_identifier => {}
            _ => return Err(WorkflowError::StateMismatch),
        }

        let state = self
            .database
            .load_initialized_state(deployment_identifier)
            .map_err(map_database_error)?;
        if !state.completion_acknowledged() {
            return Err(WorkflowError::StateMismatch);
        }

        let generation = self
            .store
            .locator()
            .map(DatabaseLocator::generation)
            .ok_or(WorkflowError::DatabaseNotSelected)?;
        let sealed = DeploymentRecord::new(
            deployment_identifier,
            LifecycleState::Initialized,
            Some(generation),
        )
        .map_err(|_| WorkflowError::Lifecycle(LifecycleError::InvalidState))?;
        self.store
            .replace_record(&RecordPersistencePermit, sealed)
            .map_err(WorkflowError::Lifecycle)?;

        Ok(SealedDeployment {
            state,
            database: self.database,
        })
    }
}

fn project(store: &LifecycleStore) -> LifecycleProjection {
    LifecycleProjection::new(store.locator().is_some())
}

macro_rules! redacted_debug {
    ($($stage:ident),+ $(,)?) => {
        $(
            impl fmt::Debug for $stage<'_> {
                fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                    formatter.write_str(concat!(stringify!($stage), "(REDACTED)"))
                }
            }
        )+
    };
}

redacted_debug!(
    WorkflowPermit,
    PendingWorkflow,
    CommittedWorkflow,
    AcknowledgedWorkflow
);
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

fn map_database_completion_error(error: DatabaseError) -> WorkflowError {
    match error {
        DatabaseError::InvalidState | DatabaseError::AlreadyInitialized => {
            WorkflowError::StateMismatch
        }
        _ => map_database_error(error),
    }
}
