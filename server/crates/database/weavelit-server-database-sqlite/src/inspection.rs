use rusqlite::Connection;
use weavelit_server_database::{
    CheckpointMetadata, DatabaseError, DatabaseInspection, DeploymentIdentifier,
    WorkflowCheckpoint, WorkflowKind,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const INSPECTION_QUERY: &str = "SELECT deployment_identifier, state, workflow_kind, checkpoint_metadata \
     FROM weavelit_lifecycle_state ORDER BY singleton LIMIT 2";

struct LifecycleRow {
    deployment_identifier: Vec<u8>,
    state: String,
    workflow_kind: Option<String>,
    checkpoint_metadata: Option<Vec<u8>>,
}

impl SqliteDatabase {
    /// Classifies durable lifecycle state for the expected Server deployment.
    pub fn inspect(
        &self,
        expected_deployment_identifier: DeploymentIdentifier,
    ) -> Result<DatabaseInspection, DatabaseError> {
        inspect_connection(&self.connection, expected_deployment_identifier)
    }
}

pub(super) fn inspect_connection(
    connection: &Connection,
    expected_deployment_identifier: DeploymentIdentifier,
) -> Result<DatabaseInspection, DatabaseError> {
    let mut statement = connection
        .prepare(INSPECTION_QUERY)
        .map_err(|error| map_sqlite_error(error, ErrorContext::Inspect))?;
    let rows = statement
        .query_map([], |row| {
            Ok(LifecycleRow {
                deployment_identifier: row.get(0)?,
                state: row.get(1)?,
                workflow_kind: row.get(2)?,
                checkpoint_metadata: row.get(3)?,
            })
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::Inspect))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Inspect))?;

    match rows.as_slice() {
        [] => Ok(DatabaseInspection::Uninitialized),
        [row] => decode_row(row, expected_deployment_identifier),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn decode_row(
    row: &LifecycleRow,
    expected_deployment_identifier: DeploymentIdentifier,
) -> Result<DatabaseInspection, DatabaseError> {
    let identifier_bytes: [u8; 16] = row
        .deployment_identifier
        .as_slice()
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    let deployment_identifier = DeploymentIdentifier::from_bytes(identifier_bytes)
        .map_err(|_| DatabaseError::IntegrityFailure)?;

    if deployment_identifier != expected_deployment_identifier {
        return Err(DatabaseError::DeploymentMismatch);
    }

    match (
        row.state.as_str(),
        row.workflow_kind.as_deref(),
        row.checkpoint_metadata.as_deref(),
    ) {
        ("pending", Some(workflow), Some(metadata)) => {
            let workflow = match workflow {
                "init" => WorkflowKind::Init,
                "restore" => WorkflowKind::Restore,
                _ => return Err(DatabaseError::IntegrityFailure),
            };
            let metadata = CheckpointMetadata::from_bytes(metadata)
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            Ok(DatabaseInspection::Pending(WorkflowCheckpoint::new(
                deployment_identifier,
                workflow,
                metadata,
            )))
        }
        ("initialized", None, None) => Ok(DatabaseInspection::Initialized {
            deployment_identifier,
        }),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}
