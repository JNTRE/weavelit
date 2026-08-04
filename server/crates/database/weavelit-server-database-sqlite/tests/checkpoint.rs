use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    ApplicationDatabase, CheckpointMetadata, DatabaseError, DatabaseInspection,
    DeploymentIdentifier, WorkflowCheckpoint, WorkflowKind,
};
use weavelit_server_database_sqlite::SqliteDatabase;

type LifecycleSnapshot = Vec<(i64, Vec<u8>, String, Option<String>, Option<Vec<u8>>)>;

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn identifier(byte: u8) -> DeploymentIdentifier {
    DeploymentIdentifier::from_bytes([byte; 16]).unwrap()
}

fn checkpoint(
    deployment_identifier: DeploymentIdentifier,
    workflow: WorkflowKind,
    metadata: &[u8],
) -> WorkflowCheckpoint {
    WorkflowCheckpoint::new(
        deployment_identifier,
        workflow,
        CheckpointMetadata::from_bytes(metadata).unwrap(),
    )
}

fn snapshot(path: &Path) -> LifecycleSnapshot {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT singleton, deployment_identifier, state, workflow_kind, \
             checkpoint_metadata FROM weavelit_lifecycle_state ORDER BY singleton",
        )
        .unwrap();
    statement
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn insert_initialized(path: &Path, deployment_identifier: DeploymentIdentifier) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (1, ?1, 'initialized', NULL, NULL)",
            [deployment_identifier.as_bytes().as_slice()],
        )
        .unwrap();
}

fn assert_redacted(error: DatabaseError, path: &Path, metadata: &[u8]) {
    let message = error.to_string();
    let lower_message = message.to_ascii_lowercase();
    assert!(!message.contains(&path.to_string_lossy().to_string()));
    assert!(!message.contains(&String::from_utf8_lossy(metadata).to_string()));
    assert!(!lower_message.contains("insert"));
    assert!(!lower_message.contains("delete"));
    assert!(!lower_message.contains("sqlite"));
    assert!(!lower_message.contains("init"));
    assert!(!lower_message.contains("restore"));
}

#[test]
fn trait_dispatch_creates_and_inspects_both_workflows() {
    for workflow in [WorkflowKind::Init, WorkflowKind::Restore] {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let deployment_identifier = identifier(1);
        let expected = checkpoint(deployment_identifier, workflow, b"workflow-metadata");
        let mut concrete = SqliteDatabase::open(&path).unwrap();
        let database: &mut dyn ApplicationDatabase = &mut concrete;

        database.create_checkpoint(&expected).unwrap();
        assert_eq!(
            database.inspect(deployment_identifier).unwrap(),
            DatabaseInspection::Pending(expected.clone())
        );
    }
}

#[test]
fn checkpoint_persists_across_reopen_without_mutation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = identifier(2);
    let expected = checkpoint(
        deployment_identifier,
        WorkflowKind::Restore,
        b"restart-metadata",
    );
    {
        let mut database = SqliteDatabase::open(&path).unwrap();
        database.create_checkpoint(&expected).unwrap();
    }
    let before = snapshot(&path);

    let database = SqliteDatabase::open(&path).unwrap();

    assert_eq!(snapshot(&path), before);
    assert_eq!(
        database.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Pending(expected)
    );
}

#[test]
fn stale_instances_allow_only_one_checkpoint_creation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = identifier(3);
    let first_checkpoint = checkpoint(deployment_identifier, WorkflowKind::Init, b"first");
    let conflicting = checkpoint(deployment_identifier, WorkflowKind::Restore, b"second");
    let mut first = SqliteDatabase::open(&path).unwrap();
    let mut stale = SqliteDatabase::open(&path).unwrap();

    first.create_checkpoint(&first_checkpoint).unwrap();
    let error = stale.create_checkpoint(&conflicting).unwrap_err();

    assert_eq!(error, DatabaseError::InvalidState);
    assert_eq!(
        stale.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Pending(first_checkpoint)
    );
}

#[test]
fn repeated_and_conflicting_creation_is_rejected_without_mutation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = identifier(4);
    let original = checkpoint(deployment_identifier, WorkflowKind::Init, b"original");
    let attempts = [
        original.clone(),
        checkpoint(deployment_identifier, WorkflowKind::Init, b"different"),
        checkpoint(deployment_identifier, WorkflowKind::Restore, b"original"),
    ];
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.create_checkpoint(&original).unwrap();
    let before = snapshot(&path);

    for attempt in attempts {
        assert_eq!(
            database.create_checkpoint(&attempt),
            Err(DatabaseError::InvalidState)
        );
        assert_eq!(snapshot(&path), before);
    }
}

#[test]
fn deployment_mismatch_rejects_checkpoint_creation_without_mutation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let persisted = checkpoint(identifier(5), WorkflowKind::Init, b"persisted-secret");
    let expected = checkpoint(identifier(6), WorkflowKind::Restore, b"different-secret");
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.create_checkpoint(&persisted).unwrap();
    let before = snapshot(&path);

    let create_error = database.create_checkpoint(&expected).unwrap_err();

    assert_eq!(create_error, DatabaseError::DeploymentMismatch);
    assert_eq!(snapshot(&path), before);
    assert_redacted(create_error, &path, b"persisted-secret");
}

#[test]
fn initialized_state_rejects_checkpoint_creation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = identifier(9);
    let expected = checkpoint(deployment_identifier, WorkflowKind::Init, b"metadata");
    let database = SqliteDatabase::open(&path).unwrap();
    drop(database);
    insert_initialized(&path, deployment_identifier);
    let mut database = SqliteDatabase::open(&path).unwrap();

    assert_eq!(
        database.create_checkpoint(&expected),
        Err(DatabaseError::AlreadyInitialized)
    );
}

#[test]
fn insert_trigger_failure_rolls_back_checkpoint_creation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER test_reject_checkpoint_insert \
             BEFORE INSERT ON weavelit_lifecycle_state \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    drop(connection);

    let error = database
        .create_checkpoint(&checkpoint(identifier(10), WorkflowKind::Init, b"secret"))
        .unwrap_err();
    drop(database);

    assert_eq!(error, DatabaseError::IntegrityFailure);
    assert!(snapshot(&path).is_empty());
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TRIGGER test_reject_checkpoint_insert;")
        .unwrap();
    assert_eq!(
        SqliteDatabase::open(&path)
            .unwrap()
            .inspect(identifier(10))
            .unwrap(),
        DatabaseInspection::Uninitialized
    );
    assert_redacted(error, &path, b"secret");
}

#[test]
fn malformed_state_is_rejected_before_any_mutation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = SqliteDatabase::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE weavelit_lifecycle_state; \
             CREATE TABLE weavelit_lifecycle_state ( \
                 singleton INTEGER, deployment_identifier BLOB, state TEXT, \
                 workflow_kind TEXT, checkpoint_metadata BLOB \
             );",
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state VALUES (1, ?1, 'broken', NULL, NULL)",
            [[12_u8; 16].as_slice()],
        )
        .unwrap();
    drop(connection);
    let before = snapshot(&path);
    let expected = checkpoint(identifier(12), WorkflowKind::Init, b"metadata");

    assert_eq!(
        database.create_checkpoint(&expected),
        Err(DatabaseError::IntegrityFailure)
    );
    assert_eq!(snapshot(&path), before);
}
