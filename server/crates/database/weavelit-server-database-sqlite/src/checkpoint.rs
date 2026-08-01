use rusqlite::{TransactionBehavior, params};
use weavelit_server_database::{
    DatabaseError, DatabaseInspection, WorkflowCheckpoint, WorkflowKind,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::inspection::inspect_connection;

impl SqliteDatabase {
    pub(super) fn create_checkpoint_atomic(
        &mut self,
        checkpoint: &WorkflowCheckpoint,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))?;

        match inspect_connection(&transaction, checkpoint.deployment_identifier())? {
            DatabaseInspection::Uninitialized => {}
            DatabaseInspection::Pending(_) => return Err(DatabaseError::InvalidState),
            DatabaseInspection::Initialized { .. } => {
                return Err(DatabaseError::AlreadyInitialized);
            }
        }

        transaction
            .execute(
                "INSERT INTO weavelit_lifecycle_state \
                 (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                 VALUES (1, ?1, 'pending', ?2, ?3)",
                params![
                    checkpoint.deployment_identifier().as_bytes().as_slice(),
                    workflow_name(checkpoint.workflow()),
                    checkpoint.metadata().as_bytes(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))
    }

    pub(super) fn reconcile_checkpoint_atomic(
        &mut self,
        expected_checkpoint: &WorkflowCheckpoint,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))?;

        match inspect_connection(&transaction, expected_checkpoint.deployment_identifier())? {
            DatabaseInspection::Uninitialized => return Err(DatabaseError::NotInitialized),
            DatabaseInspection::Pending(persisted) if persisted == *expected_checkpoint => {}
            DatabaseInspection::Pending(_) => return Err(DatabaseError::InvalidState),
            DatabaseInspection::Initialized { .. } => {
                return Err(DatabaseError::AlreadyInitialized);
            }
        }

        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))
    }

    pub(super) fn discard_checkpoint_atomic(
        &mut self,
        expected_deployment_identifier: weavelit_server_database::DeploymentIdentifier,
        expected_workflow: WorkflowKind,
    ) -> Result<(), DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))?;

        match inspect_connection(&transaction, expected_deployment_identifier)? {
            DatabaseInspection::Uninitialized => return Err(DatabaseError::NotInitialized),
            DatabaseInspection::Pending(persisted) if persisted.workflow() == expected_workflow => {
            }
            DatabaseInspection::Pending(_) => return Err(DatabaseError::InvalidState),
            DatabaseInspection::Initialized { .. } => {
                return Err(DatabaseError::AlreadyInitialized);
            }
        }

        let deleted = transaction
            .execute(
                "DELETE FROM weavelit_lifecycle_state WHERE singleton = 1",
                [],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))?;
        if deleted != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }

        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Checkpoint))
    }
}

fn workflow_name(workflow: WorkflowKind) -> &'static str {
    match workflow {
        WorkflowKind::Init => "init",
        WorkflowKind::Restore => "restore",
    }
}
