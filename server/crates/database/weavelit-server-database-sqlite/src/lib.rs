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
    WorkflowCheckpoint,
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
}
