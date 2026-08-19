use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use weavelit_server_database::{
    DatabaseError, MAX_AUDIT_TERMINAL_OBLIGATION_BYTES,
    MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES,
};
use weavelit_server_database_sqlite::SqliteDatabase;

const LEDGER_TABLE: &str = "weavelit_migration_ledger";
const LIFECYCLE_TABLE: &str = "weavelit_lifecycle_state";
const ACCOUNT_TABLE: &str = "weavelit_account";
const RECONCILIATION_TABLE: &str = "weavelit_lifecycle_reconciliation";
const SESSION_TABLE: &str = "weavelit_session";
const AUDIT_TERMINAL_OUTBOX_TABLE: &str = "weavelit_audit_terminal_outbox";
const AUDIT_TERMINAL_SUPERSESSION_TABLE: &str = "weavelit_audit_terminal_supersession";
const UPDATE_TRIGGER: &str = "weavelit_migration_ledger_reject_update";
const DELETE_TRIGGER: &str = "weavelit_migration_ledger_reject_delete";
const AUDIT_TERMINAL_MIGRATION: &str =
    include_str!("../migrations/0008_add_audit_terminal_outbox.sql");
const MIGRATION_MAX_AUDIT_TERMINAL_OBLIGATION_BYTES: usize = 50_176;
const MIGRATION_MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES: usize = 1_024;
const _: [(); MIGRATION_MAX_AUDIT_TERMINAL_OBLIGATION_BYTES] =
    [(); MAX_AUDIT_TERMINAL_OBLIGATION_BYTES];
const _: [(); MIGRATION_MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES] =
    [(); MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES];

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

    assert_eq!(first_ledger.len(), 8);
    assert_eq!(first_ledger[0].0, 1);
    assert_eq!(first_ledger[0].1, "0001_create_migration_ledger");
    assert_eq!(first_ledger[1].0, 2);
    assert_eq!(first_ledger[1].1, "0002_create_lifecycle_state");
    assert_eq!(first_ledger[2].0, 3);
    assert_eq!(first_ledger[2].1, "0003_create_application_state");
    assert_eq!(first_ledger[3].0, 4);
    assert_eq!(first_ledger[3].1, "0004_create_session_store");
    assert_eq!(first_ledger[4].0, 5);
    assert_eq!(
        first_ledger[4].1,
        "0005_add_mfa_policy_and_replay_watermark"
    );
    assert_eq!(first_ledger[5].0, 6);
    assert_eq!(first_ledger[5].1, "0006_add_lifecycle_reconciliation");
    assert_eq!(first_ledger[6].0, 7);
    assert_eq!(first_ledger[6].1, "0007_add_audit_references");
    assert_eq!(first_ledger[7].0, 8);
    assert_eq!(first_ledger[7].1, "0008_add_audit_terminal_outbox");
    assert_eq!(first_ledger[0].2.len(), 32);
    assert_eq!(first_ledger[1].2.len(), 32);
    assert_eq!(first_ledger[2].2.len(), 32);
    assert_eq!(first_ledger[3].2.len(), 32);
    assert_eq!(first_ledger[4].2.len(), 32);
    assert_eq!(first_ledger[5].2.len(), 32);
    assert_eq!(first_ledger[6].2.len(), 32);
    assert_eq!(first_ledger[7].2.len(), 32);
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
    assert_eq!(
        first_ledger[3].2,
        Sha256::digest(include_bytes!(
            "../migrations/0004_create_session_store.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[4].2,
        Sha256::digest(include_bytes!(
            "../migrations/0005_add_mfa_policy_and_replay_watermark.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[5].2,
        Sha256::digest(include_bytes!(
            "../migrations/0006_add_lifecycle_reconciliation.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[6].2,
        Sha256::digest(include_bytes!(
            "../migrations/0007_add_audit_references.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[7].2,
        Sha256::digest(include_bytes!(
            "../migrations/0008_add_audit_terminal_outbox.sql"
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
            .any(|(_, name, _)| name == RECONCILIATION_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == ACCOUNT_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == SESSION_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == AUDIT_TERMINAL_OUTBOX_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == AUDIT_TERMINAL_SUPERSESSION_TABLE)
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
            (sequence_number, identifier, checksum) VALUES (9, '0009_unknown', ?1)",
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
    assert_eq!(ledger_rows(&direct_connection(&path)).len(), 7);
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
fn audit_terminal_schema_enforces_bounds_ordering_and_append_only_dispositions() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    let first_identifier = [0x31_u8; 16];
    let second_identifier = [0x32_u8; 16];
    let binding_identifier = [0x41_u8; 16];
    let binding_version = 3_u64.to_be_bytes();

    for identifier in [first_identifier, second_identifier] {
        connection
            .execute(
                "INSERT INTO weavelit_audit_terminal_outbox \
                 (obligation_identifier, projection, binding_identifier, binding_version) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    identifier.as_slice(),
                    b"opaque-bounded-projection".as_slice(),
                    binding_identifier.as_slice(),
                    binding_version.as_slice(),
                ],
            )
            .unwrap();
    }
    assert_eq!(
        connection
            .prepare(
                "SELECT obligation_identifier FROM weavelit_audit_terminal_outbox \
                 ORDER BY sequence_number"
            )
            .unwrap()
            .query_map([], |row| row.get::<_, Vec<u8>>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        vec![first_identifier.to_vec(), second_identifier.to_vec()]
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_audit_terminal_outbox \
                 (obligation_identifier, projection, binding_identifier, binding_version) \
                 VALUES (zeroblob(16), x'01', ?1, ?2)",
                params![binding_identifier.as_slice(), binding_version.as_slice()],
            )
            .is_err()
    );
    for (identifier, projection, binding, version) in [
        (
            [0x51_u8; 16],
            Vec::new(),
            binding_identifier.to_vec(),
            binding_version.to_vec(),
        ),
        (
            [0x52_u8; 16],
            vec![0_u8; MIGRATION_MAX_AUDIT_TERMINAL_OBLIGATION_BYTES + 1],
            binding_identifier.to_vec(),
            binding_version.to_vec(),
        ),
        (
            [0x53_u8; 16],
            vec![1_u8],
            vec![0_u8; 16],
            binding_version.to_vec(),
        ),
        (
            [0x54_u8; 16],
            vec![1_u8],
            binding_identifier.to_vec(),
            vec![0_u8; 8],
        ),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO weavelit_audit_terminal_outbox \
                     (obligation_identifier, projection, binding_identifier, binding_version) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![identifier.as_slice(), projection, binding, version],
                )
                .is_err()
        );
    }
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_audit_terminal_outbox \
                 (obligation_identifier, projection, binding_identifier, binding_version) \
                 VALUES (?1, x'01', ?2, ?3)",
                params![
                    first_identifier.as_slice(),
                    binding_identifier.as_slice(),
                    binding_version.as_slice(),
                ],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE weavelit_audit_terminal_outbox SET projection = x'02' \
                 WHERE obligation_identifier = ?1",
                [first_identifier.as_slice()],
            )
            .is_err()
    );

    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_supersession \
             (original_obligation_identifier, disposition, replacement_obligation_identifier) \
             VALUES (?1, x'01', ?2)",
            params![first_identifier.as_slice(), second_identifier.as_slice()],
        )
        .unwrap();
    for (original, disposition, replacement) in [
        ([0x61_u8; 16], Vec::new(), [0x62_u8; 16]),
        (
            [0x63_u8; 16],
            vec![0_u8; MIGRATION_MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES + 1],
            [0x64_u8; 16],
        ),
        ([0x65_u8; 16], vec![1_u8], [0x65_u8; 16]),
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO weavelit_audit_terminal_supersession \
                     (original_obligation_identifier, disposition, \
                      replacement_obligation_identifier) \
                     VALUES (?1, ?2, ?3)",
                    params![original.as_slice(), disposition, replacement.as_slice()],
                )
                .is_err()
        );
    }
    assert!(
        connection
            .execute(
                "UPDATE weavelit_audit_terminal_supersession SET disposition = x'02'",
                [],
            )
            .is_err()
    );
    assert!(
        connection
            .execute("DELETE FROM weavelit_audit_terminal_supersession", [])
            .is_err()
    );
    connection
        .execute(
            "DELETE FROM weavelit_audit_terminal_outbox \
             WHERE obligation_identifier = ?1",
            [first_identifier.as_slice()],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM weavelit_audit_terminal_supersession",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1,
        "acknowledgement deletion must not delete append-only disposition history"
    );
    for table in [
        AUDIT_TERMINAL_OUTBOX_TABLE,
        AUDIT_TERMINAL_SUPERSESSION_TABLE,
    ] {
        let foreign_key_count = connection
            .prepare(&format!("PRAGMA foreign_key_list('{table}')"))
            .unwrap()
            .query_map([], |_| Ok(()))
            .unwrap()
            .count();
        assert_eq!(
            foreign_key_count, 0,
            "live Audit recovery must not depend on restorable application-state rows"
        );
    }
}

#[test]
fn audit_terminal_migration_bounds_match_the_public_contract() {
    assert!(AUDIT_TERMINAL_MIGRATION.contains(&format!(
        "length(projection) BETWEEN 1 AND {MIGRATION_MAX_AUDIT_TERMINAL_OBLIGATION_BYTES}"
    )));
    assert!(AUDIT_TERMINAL_MIGRATION.contains(&format!(
        "length(disposition) BETWEEN 1 AND \
         {MIGRATION_MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES}"
    )));
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
