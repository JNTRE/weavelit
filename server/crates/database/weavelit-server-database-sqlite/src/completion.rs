use rusqlite::{TransactionBehavior, params};
use weavelit_server_database::{
    ApplicationState, AuditReferencePersistence, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, InitializedState, ReconciliationDigest, StateIdentifier,
    WorkflowCheckpoint,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::inspection::inspect_connection;
use crate::mfa;
use crate::reconciliation;
use crate::session;
use crate::state;

impl SqliteDatabase {
    pub(super) fn complete_checkpoint_atomic(
        &mut self,
        checkpoint: &WorkflowCheckpoint,
        application_state: &ApplicationState,
        reconciliation_digest: &ReconciliationDigest,
    ) -> Result<(), DatabaseError> {
        if application_state.completion_obligation().workflow() != checkpoint.workflow() {
            return Err(DatabaseError::InvalidState);
        }

        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))?;

        match inspect_connection(&transaction, checkpoint.deployment_identifier())? {
            DatabaseInspection::Uninitialized => return Err(DatabaseError::InvalidState),
            DatabaseInspection::Initialized { .. } => {
                return Err(DatabaseError::AlreadyInitialized);
            }
            DatabaseInspection::Pending(pending) if &pending == checkpoint => {}
            DatabaseInspection::Pending(_) => return Err(DatabaseError::InvalidState),
        }

        // Live sessions are cleared inside the replacement itself, so a
        // Restore cannot leave a session that authenticates against replaced
        // state and no interruption can land between the two. Replay
        // watermarks are live in the same sense and are cleared with them.
        session::clear(&transaction)?;
        mfa::clear(&transaction)?;
        reconciliation::replace(&transaction, reconciliation_digest)?;
        state::write(&transaction, application_state)?;
        let replaced = transaction
            .execute(
                "UPDATE weavelit_lifecycle_state \
                 SET state = 'initialized', workflow_kind = NULL, checkpoint_metadata = NULL \
                 WHERE singleton = 1 AND state = 'pending' AND deployment_identifier = ?1",
                params![checkpoint.deployment_identifier().as_bytes().as_slice()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))?;
        if replaced != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }

        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))
    }

    pub(super) fn load_initialized_state_atomic(
        &mut self,
        persistence: &AuditReferencePersistence,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<InitializedState, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::State))?;

        match inspect_connection(&transaction, expected_deployment_identifier)? {
            DatabaseInspection::Initialized {
                deployment_identifier,
            } => {
                let (application_state, acknowledged) = state::read(&transaction, persistence)?;
                Ok(InitializedState::new(
                    deployment_identifier,
                    application_state,
                    acknowledged,
                ))
            }
            DatabaseInspection::Uninitialized | DatabaseInspection::Pending(_) => {
                Err(DatabaseError::NotInitialized)
            }
        }
    }

    pub(super) fn acknowledge_completion_atomic(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
        record_identifier: StateIdentifier,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))?;

        match inspect_connection(&transaction, expected_deployment_identifier)? {
            DatabaseInspection::Initialized { .. } => {}
            DatabaseInspection::Uninitialized | DatabaseInspection::Pending(_) => {
                return Err(DatabaseError::NotInitialized);
            }
        }

        let acknowledged = transaction
            .execute(
                "UPDATE weavelit_completion_obligation SET acknowledged = 1 \
                 WHERE singleton = 1 AND acknowledged = 0 AND record_id = ?1",
                params![record_identifier.as_bytes().as_slice()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))?;
        if acknowledged != 1 {
            return Err(DatabaseError::InvalidState);
        }

        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Completion))
    }
}
