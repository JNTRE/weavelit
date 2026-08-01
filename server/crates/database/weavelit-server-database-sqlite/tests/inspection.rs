use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tempfile::TempDir;
use weavelit_server_database::{
    DatabaseError, DatabaseInspection, DeploymentIdentifier, MAX_CHECKPOINT_METADATA_LENGTH,
    WorkflowKind,
};
use weavelit_server_database_sqlite::SqliteDatabase;

type SchemaSnapshot = Vec<(String, String)>;
type LedgerSnapshot = Vec<(i64, String, Vec<u8>)>;
type LifecycleSnapshot = Vec<(i64, Vec<u8>, String, Option<String>, Option<Vec<u8>>)>;
type DatabaseSnapshot = (SchemaSnapshot, LedgerSnapshot, LifecycleSnapshot);
type MalformedCase<'a> = (&'a [u8], &'a str, Option<&'a str>, Option<&'a [u8]>);

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

fn insert_state(
    path: &Path,
    deployment_identifier: DeploymentIdentifier,
    state: &str,
    workflow_kind: Option<&str>,
    checkpoint_metadata: Option<&[u8]>,
) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![
                deployment_identifier.as_bytes().as_slice(),
                state,
                workflow_kind,
                checkpoint_metadata
            ],
        )
        .unwrap();
}

fn rebuild_unconstrained_lifecycle_table(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch(
            "DROP TABLE weavelit_lifecycle_state; \
             CREATE TABLE weavelit_lifecycle_state ( \
                 singleton INTEGER, \
                 deployment_identifier BLOB, \
                 state TEXT, \
                 workflow_kind TEXT, \
                 checkpoint_metadata BLOB \
             );",
        )
        .unwrap();
    connection
}

fn insert_raw_state(
    connection: &Connection,
    singleton: i64,
    deployment_identifier: &[u8],
    state: &str,
    workflow_kind: Option<&str>,
    checkpoint_metadata: Option<&[u8]>,
) {
    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                singleton,
                deployment_identifier,
                state,
                workflow_kind,
                checkpoint_metadata
            ],
        )
        .unwrap();
}

fn assert_malformed_fixture(
    deployment_identifier: &[u8],
    state: &str,
    workflow_kind: Option<&str>,
    checkpoint_metadata: Option<&[u8]>,
) {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let database = SqliteDatabase::open(&path).unwrap();
    let connection = rebuild_unconstrained_lifecycle_table(&path);
    insert_raw_state(
        &connection,
        1,
        deployment_identifier,
        state,
        workflow_kind,
        checkpoint_metadata,
    );
    drop(connection);

    let error = database.inspect(identifier(9)).unwrap_err();

    assert_eq!(error, DatabaseError::IntegrityFailure);
    assert_redacted(error, &path);
}

fn assert_redacted(error: DatabaseError, path: &Path) {
    let message = error.to_string();
    let lower_message = message.to_ascii_lowercase();
    assert!(!message.contains(&path.to_string_lossy().to_string()));
    assert!(!lower_message.contains("select"));
    assert!(!lower_message.contains("pending"));
    assert!(!lower_message.contains("initialized"));
    assert!(!lower_message.contains("restore"));
    assert!(!lower_message.contains("metadata"));
}

fn snapshot(path: &Path) -> DatabaseSnapshot {
    let connection = Connection::open(path).unwrap();
    let schema = {
        let mut statement = connection
            .prepare(
                "SELECT name, sql FROM sqlite_schema \
                 WHERE name GLOB 'weavelit_*' ORDER BY type, name",
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let ledger = {
        let mut statement = connection
            .prepare(
                "SELECT sequence_number, identifier, checksum \
                 FROM weavelit_migration_ledger ORDER BY sequence_number",
            )
            .unwrap();
        statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };
    let state = {
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
    };
    (schema, ledger, state)
}

#[test]
fn fresh_database_is_uninitialized() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let database = SqliteDatabase::open(&path).unwrap();

    assert_eq!(
        database.inspect(identifier(1)).unwrap(),
        DatabaseInspection::Uninitialized
    );
}

#[test]
fn pending_init_state_returns_exact_checkpoint() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let expected_identifier = identifier(2);
    drop(SqliteDatabase::open(&path).unwrap());
    insert_state(
        &path,
        expected_identifier,
        "pending",
        Some("init"),
        Some(b"workflow-metadata"),
    );
    let database = SqliteDatabase::open(&path).unwrap();

    let DatabaseInspection::Pending(checkpoint) = database.inspect(expected_identifier).unwrap()
    else {
        panic!("expected pending state");
    };
    assert_eq!(checkpoint.deployment_identifier(), expected_identifier);
    assert_eq!(checkpoint.workflow(), WorkflowKind::Init);
    assert_eq!(checkpoint.metadata().as_bytes(), b"workflow-metadata");
}

#[test]
fn initialized_state_returns_deployment_binding() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let expected_identifier = identifier(3);
    drop(SqliteDatabase::open(&path).unwrap());
    insert_state(&path, expected_identifier, "initialized", None, None);
    let database = SqliteDatabase::open(&path).unwrap();

    assert_eq!(
        database.inspect(expected_identifier).unwrap(),
        DatabaseInspection::Initialized {
            deployment_identifier: expected_identifier,
        }
    );
}

#[test]
fn pending_restore_supports_empty_and_maximum_metadata() {
    for metadata in [Vec::new(), vec![7_u8; MAX_CHECKPOINT_METADATA_LENGTH]] {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let expected_identifier = identifier(4);
        drop(SqliteDatabase::open(&path).unwrap());
        insert_state(
            &path,
            expected_identifier,
            "pending",
            Some("restore"),
            Some(&metadata),
        );
        let database = SqliteDatabase::open(&path).unwrap();

        let DatabaseInspection::Pending(checkpoint) =
            database.inspect(expected_identifier).unwrap()
        else {
            panic!("expected pending state");
        };
        assert_eq!(checkpoint.workflow(), WorkflowKind::Restore);
        assert_eq!(checkpoint.metadata().as_bytes(), metadata);
    }
}

#[test]
fn deployment_mismatch_precedes_state_interpretation_and_is_redacted() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let database = SqliteDatabase::open(&path).unwrap();
    let connection = rebuild_unconstrained_lifecycle_table(&path);
    insert_raw_state(
        &connection,
        1,
        &[8_u8; 16],
        "unknown-sensitive-state",
        None,
        None,
    );
    drop(connection);

    let error = database.inspect(identifier(9)).unwrap_err();

    assert_eq!(error, DatabaseError::DeploymentMismatch);
    assert_redacted(error, &path);
    assert!(!error.to_string().contains("unknown-sensitive-state"));
}

#[test]
fn malformed_persisted_shapes_fail_integrity_validation() {
    let valid_identifier = [9_u8; 16];
    let oversized_metadata = vec![0_u8; MAX_CHECKPOINT_METADATA_LENGTH + 1];
    let cases: &[MalformedCase<'_>] = &[
        (&[9_u8; 15], "initialized", None, None),
        (&[0_u8; 16], "initialized", None, None),
        (&valid_identifier, "unknown", None, None),
        (&valid_identifier, "pending", None, Some(b"metadata")),
        (
            &valid_identifier,
            "pending",
            Some("unknown"),
            Some(b"metadata"),
        ),
        (&valid_identifier, "pending", Some("init"), None),
        (
            &valid_identifier,
            "pending",
            Some("restore"),
            Some(&oversized_metadata),
        ),
        (&valid_identifier, "initialized", Some("init"), None),
        (&valid_identifier, "initialized", None, Some(b"metadata")),
    ];

    for (identifier, state, workflow, metadata) in cases {
        assert_malformed_fixture(identifier, state, *workflow, *metadata);
    }
}

#[test]
fn duplicate_lifecycle_rows_fail_cardinality_validation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let database = SqliteDatabase::open(&path).unwrap();
    let connection = rebuild_unconstrained_lifecycle_table(&path);
    insert_raw_state(&connection, 1, &[9_u8; 16], "initialized", None, None);
    insert_raw_state(&connection, 2, &[9_u8; 16], "initialized", None, None);
    drop(connection);

    let error = database.inspect(identifier(9)).unwrap_err();

    assert_eq!(error, DatabaseError::IntegrityFailure);
    assert_redacted(error, &path);
}

#[test]
fn inspection_is_stable_across_restart_and_does_not_mutate_database() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let expected_identifier = identifier(6);
    drop(SqliteDatabase::open(&path).unwrap());
    insert_state(
        &path,
        expected_identifier,
        "pending",
        Some("restore"),
        Some(b"restart-metadata"),
    );
    let before = snapshot(&path);

    let first = {
        let database = SqliteDatabase::open(&path).unwrap();
        database.inspect(expected_identifier).unwrap()
    };
    let second = {
        let database = SqliteDatabase::open(&path).unwrap();
        database.inspect(expected_identifier).unwrap()
    };

    assert_eq!(first, second);
    assert_eq!(snapshot(&path), before);
}
