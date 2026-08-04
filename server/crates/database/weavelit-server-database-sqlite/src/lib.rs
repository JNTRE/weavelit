#![forbid(unsafe_code)]

//! SQLite implementation of the Weavelit Application Database contract.

mod checkpoint;
mod connection;
mod error;
mod inspection;
mod migrations;

pub use connection::{RetainedSqliteInspection, SqliteDatabase};

use weavelit_server_database::{
    ApplicationDatabase, DatabaseError, DatabaseInspection, DeploymentIdentifier,
    WorkflowCheckpoint, WorkflowKind,
};

impl ApplicationDatabase for SqliteDatabase {
    fn inspect(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        SqliteDatabase::inspect(self, expected_deployment_identifier)
    }

    fn create_checkpoint(&mut self, checkpoint: &WorkflowCheckpoint) -> Result<(), DatabaseError> {
        self.create_checkpoint_atomic(checkpoint)
    }

    fn reconcile_checkpoint(
        &mut self,
        expected_checkpoint: &WorkflowCheckpoint,
    ) -> Result<(), DatabaseError> {
        self.reconcile_checkpoint_atomic(expected_checkpoint)
    }

    fn discard_checkpoint(
        &mut self,
        expected_deployment_identifier: DeploymentIdentifier,
        expected_workflow: WorkflowKind,
    ) -> Result<(), DatabaseError> {
        self.discard_checkpoint_atomic(expected_deployment_identifier, expected_workflow)
    }
}
