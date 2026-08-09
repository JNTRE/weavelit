use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use weavelit_server_database::DatabaseError;
use weavelit_server_database_sqlite::SqliteDatabase;

const LEDGER_TABLE: &str = "weavelit_migration_ledger";
const LIFECYCLE_TABLE: &str = "weavelit_lifecycle_state";
const ACCOUNT_TABLE: &str = "weavelit_account";
const UPDATE_TRIGGER: &str = "weavelit_migration_ledger_reject_update";
const DELETE_TRIGGER: &str = "weavelit_migration_ledger_reject_delete";

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn bootstrap(path: &Path) {
    drop(SqliteDatabase::open(path).unwrap());
}

fn direct_connection(path: &Path) -> Connection {
    Connection::open(path).unwrap()
}

fn open_error(path: &Path) -> DatabaseError {
    match SqliteDatabase::open(path) {
        Ok(_) => panic!("database open should fail integrity validation"),
        Err(error) => error,
    }
}

fn ledger_rows(connection: &Connection) -> Vec<(i64, String, Vec<u8>)> {
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
}

fn schema_rows(connection: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, sql FROM sqlite_schema \
             WHERE name GLOB 'weavelit_*' ORDER BY type, name",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}

fn disable_ledger_immutability(connection: &Connection) {
    connection
        .execute_batch(&format!(
            "DROP TRIGGER {UPDATE_TRIGGER}; DROP TRIGGER {DELETE_TRIGGER};"
        ))
        .unwrap();
}

fn assert_integrity_failure_is_redacted(error: DatabaseError, path: &Path) {
    assert_eq!(error, DatabaseError::IntegrityFailure);
    let message = error.to_string();
    assert!(!message.contains(&path.to_string_lossy().to_string()));
    assert!(!message.to_ascii_lowercase().contains("sqlite"));
    assert!(!message.to_ascii_lowercase().contains("migration"));
    assert!(!message.contains("CREATE TABLE"));
}

#[test]
fn fresh_open_applies_ordered_migrations_and_reopen_is_idempotent() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);

    let connection = direct_connection(&path);
    let first_ledger = ledger_rows(&connection);
    let first_schema = schema_rows(&connection);
    drop(connection);

    assert_eq!(first_ledger.len(), 3);
    assert_eq!(first_ledger[0].0, 1);
    assert_eq!(first_ledger[0].1, "0001_create_migration_ledger");
    assert_eq!(first_ledger[1].0, 2);
    assert_eq!(first_ledger[1].1, "0002_create_lifecycle_state");
    assert_eq!(first_ledger[2].0, 3);
    assert_eq!(first_ledger[2].1, "0003_create_application_state");
    assert_eq!(first_ledger[0].2.len(), 32);
    assert_eq!(first_ledger[1].2.len(), 32);
    assert_eq!(first_ledger[2].2.len(), 32);
    assert_eq!(
        first_ledger[0].2,
        Sha256::digest(include_bytes!(
            "../migrations/0001_create_migration_ledger.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[1].2,
        Sha256::digest(include_bytes!(
            "../migrations/0002_create_lifecycle_state.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[2].2,
        Sha256::digest(include_bytes!(
            "../migrations/0003_create_application_state.sql"
        ))
        .to_vec()
    );
    assert!(first_schema.iter().any(|(_, name, _)| name == LEDGER_TABLE));
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == LIFECYCLE_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == ACCOUNT_TABLE)
    );

    bootstrap(&path);
    let connection = direct_connection(&path);
    assert_eq!(ledger_rows(&connection), first_ledger);
    assert_eq!(schema_rows(&connection), first_schema);
}

#[test]
fn checksum_mismatch_is_rejected_without_schema_change() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    disable_ledger_immutability(&connection);
    connection
        .execute(
            "UPDATE weavelit_migration_ledger SET checksum = ?1 WHERE sequence_number = 1",
            [vec![0_u8; 32]],
        )
        .unwrap();
    let schema_before = schema_rows(&connection);
    drop(connection);

    let error = open_error(&path);

    assert_integrity_failure_is_redacted(error, &path);
    assert_eq!(schema_rows(&direct_connection(&path)), schema_before);
}

#[test]
fn unknown_extra_history_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    connection
        .execute(
            "INSERT INTO weavelit_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (4, '0004_unknown', ?1)",
            [vec![0_u8; 32]],
        )
        .unwrap();
    drop(connection);

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}

#[test]
fn missing_applied_history_is_rejected_without_new_ledger_row() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    disable_ledger_immutability(&connection);
    connection
        .execute(
            "DELETE FROM weavelit_migration_ledger WHERE sequence_number = 2",
            [],
        )
        .unwrap();
    drop(connection);

    assert_integrity_failure_is_redacted(open_error(&path), &path);
    assert_eq!(ledger_rows(&direct_connection(&path)).len(), 2);
}

#[test]
fn reordered_history_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    disable_ledger_immutability(&connection);
    connection
        .execute_batch(
            "UPDATE weavelit_migration_ledger SET identifier = 'temporary' \
             WHERE sequence_number = 1; \
             UPDATE weavelit_migration_ledger SET identifier = '0001_create_migration_ledger' \
             WHERE sequence_number = 2; \
             UPDATE weavelit_migration_ledger SET identifier = '0002_create_lifecycle_state' \
             WHERE sequence_number = 1;",
        )
        .unwrap();
    drop(connection);

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}

#[test]
fn missing_ledger_in_nonempty_database_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    connection
        .execute_batch("DROP TABLE weavelit_migration_ledger;")
        .unwrap();
    drop(connection);

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}

#[test]
fn lifecycle_schema_represents_valid_states_and_rejects_invalid_shapes() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    let deployment_identifier = [7_u8; 16];

    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (1, ?1, 'pending', 'init', ?2)",
            params![deployment_identifier.as_slice(), b"metadata"],
        )
        .unwrap();
    connection
        .execute("DELETE FROM weavelit_lifecycle_state", [])
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_lifecycle_state \
             (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
             VALUES (1, ?1, 'initialized', NULL, NULL)",
            [deployment_identifier.as_slice()],
        )
        .unwrap();

    let invalid_identifier = connection.execute(
        "INSERT OR REPLACE INTO weavelit_lifecycle_state \
         (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
         VALUES (1, ?1, 'initialized', NULL, NULL)",
        [vec![0_u8; 15]],
    );
    let invalid_initialized_shape = connection.execute(
        "INSERT OR REPLACE INTO weavelit_lifecycle_state \
         (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
         VALUES (1, ?1, 'initialized', 'init', NULL)",
        [deployment_identifier.as_slice()],
    );
    let oversized_metadata = connection.execute(
        "INSERT OR REPLACE INTO weavelit_lifecycle_state \
         (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
         VALUES (1, ?1, 'pending', 'restore', ?2)",
        params![deployment_identifier.as_slice(), vec![0_u8; 4097]],
    );

    assert!(invalid_identifier.is_err());
    assert!(invalid_initialized_shape.is_err());
    assert!(oversized_metadata.is_err());
}

#[test]
fn dropped_migrated_table_is_rejected_on_reopen() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    direct_connection(&path)
        .execute_batch("DROP TABLE weavelit_lifecycle_state;")
        .unwrap();

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}

#[test]
fn altered_migrated_constraints_are_rejected_on_reopen() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    direct_connection(&path)
        .execute_batch(
            "DROP TABLE weavelit_lifecycle_state; \
             CREATE TABLE weavelit_lifecycle_state ( \
                 singleton INTEGER PRIMARY KEY, \
                 deployment_identifier BLOB, \
                 state TEXT, \
                 workflow_kind TEXT, \
                 checkpoint_metadata BLOB \
             );",
        )
        .unwrap();

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}

#[test]
fn added_trigger_on_migrated_table_is_rejected_on_reopen() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    direct_connection(&path)
        .execute_batch(
            "CREATE TRIGGER rewrite_checkpoint_metadata \
             AFTER INSERT ON weavelit_lifecycle_state \
             BEGIN \
                 UPDATE weavelit_lifecycle_state \
                 SET checkpoint_metadata = X'00' WHERE singleton = NEW.singleton; \
             END;",
        )
        .unwrap();

    assert_integrity_failure_is_redacted(open_error(&path), &path);
}
