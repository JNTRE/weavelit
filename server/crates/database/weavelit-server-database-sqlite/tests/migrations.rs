use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use weavelit_server_database::DatabaseError;
use weavelit_server_database_sqlite::SqliteDatabase;

const LEDGER_TABLE: &str = "weavelit_migration_ledger";
const LIFECYCLE_TABLE: &str = "weavelit_lifecycle_state";
const ACCOUNT_TABLE: &str = "weavelit_account";
const AUDIT_TERMINAL_OBLIGATION_TABLE: &str = "weavelit_audit_terminal_obligation";
const AUDIT_TERMINAL_SUPERSESSION_TABLE: &str = "weavelit_audit_terminal_supersession";
const LOG_CONFIGURATION_GENERATION_TABLE: &str = "weavelit_log_configuration_generation";
const LOG_CONFIGURATION_CURRENT_TABLE: &str = "weavelit_log_configuration_current_generation";
const LOG_CONFIGURATION_AUDIT_REFERENCE_TABLE: &str = "weavelit_log_configuration_audit_reference";
const ACCOUNT_PUBLIC_IDENTITY_TABLE: &str = "weavelit_account_public_identity";
const RECONCILIATION_TABLE: &str = "weavelit_lifecycle_reconciliation";
const SESSION_TABLE: &str = "weavelit_session";
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

fn create_0008_database(path: &Path) {
    let connection = Connection::open(path).unwrap();
    for (sequence, identifier, sql) in [
        (
            1_i64,
            "0001_create_migration_ledger",
            include_str!("../migrations/0001_create_migration_ledger.sql"),
        ),
        (
            2,
            "0002_create_lifecycle_state",
            include_str!("../migrations/0002_create_lifecycle_state.sql"),
        ),
        (
            3,
            "0003_create_application_state",
            include_str!("../migrations/0003_create_application_state.sql"),
        ),
        (
            4,
            "0004_create_session_store",
            include_str!("../migrations/0004_create_session_store.sql"),
        ),
        (
            5,
            "0005_add_mfa_policy_and_replay_watermark",
            include_str!("../migrations/0005_add_mfa_policy_and_replay_watermark.sql"),
        ),
        (
            6,
            "0006_add_lifecycle_reconciliation",
            include_str!("../migrations/0006_add_lifecycle_reconciliation.sql"),
        ),
        (
            7,
            "0007_add_audit_references",
            include_str!("../migrations/0007_add_audit_references.sql"),
        ),
        (
            8,
            "0008_add_audit_terminal_recovery",
            include_str!("../migrations/0008_add_audit_terminal_recovery.sql"),
        ),
    ] {
        connection.execute_batch(sql).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                params![
                    sequence,
                    identifier,
                    Sha256::digest(sql.as_bytes()).as_slice()
                ],
            )
            .unwrap();
    }
}

fn create_0009_database(path: &Path) {
    create_0008_database(path);
    let connection = Connection::open(path).unwrap();
    let sql = include_str!("../migrations/0009_add_log_configuration_generations.sql");
    connection.execute_batch(sql).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (9, ?1, ?2)",
            params![
                "0009_add_log_configuration_generations",
                Sha256::digest(sql.as_bytes()).as_slice()
            ],
        )
        .unwrap();
}

fn create_0010_database(path: &Path) {
    create_0009_database(path);
    let connection = Connection::open(path).unwrap();
    let sql = include_str!("../migrations/0010_migrate_totp_component_enablement.sql");
    connection.execute_batch(sql).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (10, ?1, ?2)",
            params![
                "0010_migrate_totp_component_enablement",
                Sha256::digest(sql.as_bytes()).as_slice()
            ],
        )
        .unwrap();
}

fn create_0011_database(path: &Path) {
    create_0010_database(path);
    let connection = Connection::open(path).unwrap();
    let sql = include_str!("../migrations/0011_add_log_configuration_audit_references.sql");
    connection.execute_batch(sql).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (11, ?1, ?2)",
            params![
                "0011_add_log_configuration_audit_references",
                Sha256::digest(sql.as_bytes()).as_slice()
            ],
        )
        .unwrap();
}

fn create_0012_database(path: &Path) {
    create_0011_database(path);
    let connection = Connection::open(path).unwrap();
    let sql = include_str!("../migrations/0012_add_account_public_identities.sql");
    connection.execute_batch(sql).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_migration_ledger \
             (sequence_number, identifier, checksum) VALUES (12, ?1, ?2)",
            params![
                "0012_add_account_public_identities",
                Sha256::digest(sql.as_bytes()).as_slice()
            ],
        )
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

    assert_eq!(first_ledger.len(), 13);
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
    assert_eq!(first_ledger[7].1, "0008_add_audit_terminal_recovery");
    assert_eq!(first_ledger[8].0, 9);
    assert_eq!(first_ledger[8].1, "0009_add_log_configuration_generations");
    assert_eq!(first_ledger[9].0, 10);
    assert_eq!(first_ledger[9].1, "0010_migrate_totp_component_enablement");
    assert_eq!(first_ledger[10].0, 11);
    assert_eq!(
        first_ledger[10].1,
        "0011_add_log_configuration_audit_references"
    );
    assert_eq!(first_ledger[11].0, 12);
    assert_eq!(first_ledger[11].1, "0012_add_account_public_identities");
    assert_eq!(first_ledger[12].0, 13);
    assert_eq!(first_ledger[12].1, "0013_add_account_credential_state");
    assert_eq!(first_ledger[0].2.len(), 32);
    assert_eq!(first_ledger[1].2.len(), 32);
    assert_eq!(first_ledger[2].2.len(), 32);
    assert_eq!(first_ledger[3].2.len(), 32);
    assert_eq!(first_ledger[4].2.len(), 32);
    assert_eq!(first_ledger[5].2.len(), 32);
    assert_eq!(first_ledger[6].2.len(), 32);
    assert_eq!(first_ledger[7].2.len(), 32);
    assert_eq!(first_ledger[8].2.len(), 32);
    assert_eq!(first_ledger[9].2.len(), 32);
    assert_eq!(first_ledger[10].2.len(), 32);
    assert_eq!(first_ledger[11].2.len(), 32);
    assert_eq!(first_ledger[12].2.len(), 32);
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
            "../migrations/0008_add_audit_terminal_recovery.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[8].2,
        Sha256::digest(include_bytes!(
            "../migrations/0009_add_log_configuration_generations.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[9].2,
        Sha256::digest(include_bytes!(
            "../migrations/0010_migrate_totp_component_enablement.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[10].2,
        Sha256::digest(include_bytes!(
            "../migrations/0011_add_log_configuration_audit_references.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[11].2,
        Sha256::digest(include_bytes!(
            "../migrations/0012_add_account_public_identities.sql"
        ))
        .to_vec()
    );
    assert_eq!(
        first_ledger[12].2,
        Sha256::digest(include_bytes!(
            "../migrations/0013_add_account_credential_state.sql"
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
            .any(|(_, name, _)| name == AUDIT_TERMINAL_OBLIGATION_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == AUDIT_TERMINAL_SUPERSESSION_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == LOG_CONFIGURATION_GENERATION_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == LOG_CONFIGURATION_CURRENT_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == LOG_CONFIGURATION_AUDIT_REFERENCE_TABLE)
    );
    assert!(
        first_schema
            .iter()
            .any(|(_, name, _)| name == ACCOUNT_PUBLIC_IDENTITY_TABLE)
    );

    bootstrap(&path);
    let connection = direct_connection(&path);
    assert_eq!(ledger_rows(&connection), first_ledger);
    assert_eq!(schema_rows(&connection), first_schema);
}

#[test]
fn populated_0008_database_upgrades_and_backfills_version_one() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    create_0008_database(&path);
    let configuration_id = [0x61_u8; 16];
    let connection = direct_connection(&path);
    connection
        .execute(
            "INSERT INTO weavelit_log_module_configuration \
             (configuration_id, module, name, enabled) \
             VALUES (?1, 'log-sqlite', 'existing', 1)",
            [configuration_id.as_slice()],
        )
        .unwrap();
    for (key, value) in [("path", "logs.db"), ("retention", "30d")] {
        connection
            .execute(
                "INSERT INTO weavelit_log_module_setting \
                 (configuration_id, setting_key, setting_value) VALUES (?1, ?2, ?3)",
                params![configuration_id.as_slice(), key, value],
            )
            .unwrap();
    }
    for log_type in ["system", "audit"] {
        connection
            .execute(
                "INSERT INTO weavelit_log_assignment (log_type, configuration_id) \
                 VALUES (?1, ?2)",
                params![log_type, configuration_id.as_slice()],
            )
            .unwrap();
    }
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());

    let connection = direct_connection(&path);
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    assert_eq!(ledger_rows(&connection).len(), 13);
    let generation: (Vec<u8>, Vec<u8>, String, String, i64) = connection
        .query_row(
            "SELECT configuration_id, generation_version, module, name, enabled \
             FROM weavelit_log_configuration_generation",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(generation.0, configuration_id);
    assert_eq!(generation.1, 1_u64.to_be_bytes());
    assert_eq!(generation.2, "log-sqlite");
    assert_eq!(generation.3, "existing");
    assert_eq!(generation.4, 1);
    assert_eq!(
        connection
            .prepare(
                "SELECT setting_key, setting_value \
                 FROM weavelit_log_configuration_generation_setting ORDER BY setting_key",
            )
            .unwrap()
            .query_map([], |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?
            )))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        [
            ("path".to_owned(), "logs.db".to_owned()),
            ("retention".to_owned(), "30d".to_owned())
        ]
    );
    assert_eq!(
        connection
            .prepare(
                "SELECT log_type FROM weavelit_log_configuration_generation_log_type \
                 ORDER BY log_type",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        ["audit", "system"]
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT configuration_id, generation_version \
                 FROM weavelit_log_configuration_current_generation",
                [],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?)),
            )
            .unwrap(),
        (configuration_id.to_vec(), 1_u64.to_be_bytes().to_vec())
    );
}

#[test]
fn populated_0010_database_backfills_immutable_globally_unique_configuration_references() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    create_0010_database(&path);
    let connection = direct_connection(&path);
    let configuration_id = [0x61_u8; 16];
    connection
        .execute(
            "INSERT INTO weavelit_log_module_configuration \
             (configuration_id, module, name, enabled) \
             VALUES (?1, 'log-sqlite', 'existing', 1)",
            [configuration_id.as_slice()],
        )
        .unwrap();
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());

    let connection = direct_connection(&path);
    let reference: String = connection
        .query_row(
            "SELECT audit_reference FROM weavelit_log_configuration_audit_reference \
             WHERE configuration_id = ?1",
            [configuration_id.as_slice()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(reference.len(), 35);
    assert!(reference.starts_with("ar-"));
    assert!(
        reference[3..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    );
    assert_ne!(&reference[3..], "00000000000000000000000000000000");
    assert!(
        connection
            .execute(
                "UPDATE weavelit_log_configuration_audit_reference \
                 SET audit_reference = 'ar-11111111111111111111111111111111'",
                [],
            )
            .is_err()
    );

    let account_id = [0x62_u8; 16];
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, 'collision-account', NULL, 1, 0)",
            [account_id.as_slice()],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference \
                 (account_id, audit_reference) VALUES (?1, ?2)",
                params![account_id.as_slice(), reference],
            )
            .is_err()
    );

    let second_configuration = [0x63_u8; 16];
    connection
        .execute(
            "INSERT INTO weavelit_log_module_configuration \
             (configuration_id, module, name, enabled) \
             VALUES (?1, 'log-sqlite', 'second', 0)",
            [second_configuration.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_audit_reference \
             (account_id, audit_reference) \
             VALUES (?1, 'ar-22222222222222222222222222222222')",
            [account_id.as_slice()],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_audit_reference \
                 (configuration_id, audit_reference) \
                 VALUES (?1, 'ar-22222222222222222222222222222222')",
                [second_configuration.as_slice()],
            )
            .is_err()
    );
}

#[test]
fn populated_0011_database_backfills_immutable_unique_account_public_identities() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    create_0011_database(&path);
    let accounts = [
        (
            [0x31_u8; 16],
            "管理者-équipe",
            Some("運用担当"),
            1_i64,
            0_i64,
        ),
        ([0x32_u8; 16], "second-admin", None, 0_i64, 1_i64),
    ];
    let connection = direct_connection(&path);
    for (identifier, username, display_name, active, mfa_required) in &accounts {
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    identifier.as_slice(),
                    username,
                    display_name,
                    active,
                    mfa_required
                ],
            )
            .unwrap();
    }
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());

    let connection = direct_connection(&path);
    assert_eq!(ledger_rows(&connection).len(), 13);
    let identities = connection
        .prepare(
            "SELECT account_id, public_identifier \
             FROM weavelit_account_public_identity ORDER BY account_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(identities.len(), accounts.len());
    assert_ne!(identities[0].1, identities[1].1);
    for ((account_id, public_identifier), (expected_id, _, _, _, _)) in
        identities.iter().zip(accounts.iter())
    {
        assert_eq!(account_id, expected_id);
        assert_eq!(public_identifier.len(), 16);
        assert_ne!(public_identifier, &[0; 16]);
        assert_ne!(public_identifier, account_id);
    }
    assert_eq!(
        connection
            .prepare(
                "SELECT account_id, username, display_name, active, mfa_required \
                 FROM weavelit_account ORDER BY account_id",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap(),
        accounts
            .iter()
            .map(
                |(identifier, username, display_name, active, mfa_required)| (
                    identifier.to_vec(),
                    (*username).to_owned(),
                    display_name.map(str::to_owned),
                    *active,
                    *mfa_required,
                )
            )
            .collect::<Vec<_>>()
    );
    assert!(
        connection
            .execute(
                "UPDATE weavelit_account_public_identity \
                 SET public_identifier = ?1 WHERE account_id = ?2",
                params![[0x41_u8; 16].as_slice(), accounts[0].0.as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "DELETE FROM weavelit_account_public_identity WHERE account_id = ?1",
                [accounts[0].0.as_slice()],
            )
            .is_err()
    );
    let third_account = [0x33_u8; 16];
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, 'third-admin', NULL, 1, 0)",
            [third_account.as_slice()],
        )
        .unwrap();
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) VALUES (?1, ?2)",
                params![third_account.as_slice(), identities[0].1.as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) VALUES (?1, zeroblob(16))",
                [third_account.as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) \
                 VALUES (x'44444444444444444444444444444444', \
                         x'45454545454545454545454545454545')",
                [],
            )
            .is_err()
    );
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());
    let connection = direct_connection(&path);
    let reopened = connection
        .prepare(
            "SELECT account_id, public_identifier \
             FROM weavelit_account_public_identity ORDER BY account_id",
        )
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(reopened, identities);
}

#[test]
fn populated_0012_database_backfills_credential_state_and_preserves_sessions() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    create_0012_database(&path);
    let account_id = [0x51_u8; 16];
    let token_hash = [0x61_u8; 32];
    let csrf_hash = [0x62_u8; 32];
    let connection = direct_connection(&path);
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, 'legacy-account', 'Legacy Account', 1, 1)",
            [account_id.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, 'web-ui', 1000, 1000, 43201000)",
            params![
                token_hash.as_slice(),
                csrf_hash.as_slice(),
                account_id.as_slice()
            ],
        )
        .unwrap();
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());

    let connection = direct_connection(&path);
    assert_eq!(ledger_rows(&connection).len(), 13);
    assert_eq!(
        connection
            .query_row(
                "SELECT username, active, mfa_required, credential_revision, \
                 must_change_password, temporary_credential_expires_at_milliseconds \
                 FROM weavelit_account WHERE account_id = ?1",
                [account_id.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Vec<u8>>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .unwrap(),
        (
            "legacy-account".to_owned(),
            1,
            1,
            1_u64.to_be_bytes().to_vec(),
            0,
            None,
        )
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT csrf_hash, account_id, issued_at_milliseconds, \
                 absolute_expires_at_milliseconds FROM weavelit_session WHERE token_hash = ?1",
                [token_hash.as_slice()],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .unwrap(),
        (csrf_hash.to_vec(), account_id.to_vec(), 1_000, 43_201_000)
    );

    for statement in [
        "UPDATE weavelit_account SET credential_revision = zeroblob(8)",
        "UPDATE weavelit_account SET credential_revision = x'01020304050607'",
        "UPDATE weavelit_account SET credential_revision = 1",
        "UPDATE weavelit_account SET must_change_password = 1",
        "UPDATE weavelit_account SET temporary_credential_expires_at_milliseconds = 0",
        "UPDATE weavelit_account SET must_change_password = 1, \
         temporary_credential_expires_at_milliseconds = -1",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "for {statement}"
        );
    }
    connection
        .execute(
            "UPDATE weavelit_account SET credential_revision = ?1, must_change_password = 1, \
             temporary_credential_expires_at_milliseconds = 0",
            [u64::MAX.to_be_bytes().as_slice()],
        )
        .unwrap();
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());
    let connection = direct_connection(&path);
    assert_eq!(
        connection
            .query_row(
                "SELECT credential_revision, must_change_password, \
                 temporary_credential_expires_at_milliseconds FROM weavelit_account",
                [],
                |row| {
                    Ok((
                        row.get::<_, Vec<u8>>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .unwrap(),
        (u64::MAX.to_be_bytes().to_vec(), 1, Some(0))
    );
}

#[test]
fn initialized_legacy_totp_enablement_migrates_to_one_canonical_authority() {
    for (legacy, existing_canonical, expected) in [
        (Some("true"), None, "true"),
        (Some("false"), None, "false"),
        (Some("yes"), None, "false"),
        (None, None, "false"),
        (Some("true"), Some("false"), "true"),
        (Some("false"), Some("true"), "false"),
    ] {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        create_0009_database(&path);
        let connection = direct_connection(&path);
        connection
            .execute(
                "INSERT INTO weavelit_lifecycle_state \
                 (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                 VALUES (1, ?1, 'initialized', NULL, NULL)",
                [[0x51_u8; 16].as_slice()],
            )
            .unwrap();
        if let Some(value) = legacy {
            connection
                .execute(
                    "INSERT INTO weavelit_configuration \
                     (component, setting_key, setting_value) \
                     VALUES ('mfa.totp', 'enabled', ?1)",
                    [value],
                )
                .unwrap();
        }
        if let Some(value) = existing_canonical {
            connection
                .execute(
                    "INSERT INTO weavelit_configuration \
                     (component, setting_key, setting_value) \
                     VALUES ('totp', 'mfa-module.enabled', ?1)",
                    [value],
                )
                .unwrap();
        }
        drop(connection);

        drop(SqliteDatabase::open(&path).unwrap());
        drop(SqliteDatabase::open(&path).unwrap());

        let connection = direct_connection(&path);
        assert_eq!(ledger_rows(&connection).len(), 13);
        assert_eq!(
            connection
                .query_row(
                    "SELECT setting_value FROM weavelit_configuration \
                     WHERE component = 'totp' AND setting_key = 'mfa-module.enabled'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            expected
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_configuration \
                     WHERE component = 'mfa.totp' AND setting_key = 'enabled'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn uninitialized_migration_does_not_seed_or_remove_totp_enablement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    create_0009_database(&path);
    let connection = direct_connection(&path);
    connection
        .execute(
            "INSERT INTO weavelit_configuration \
             (component, setting_key, setting_value) VALUES ('mfa.totp', 'enabled', 'true')",
            [],
        )
        .unwrap();
    drop(connection);

    drop(SqliteDatabase::open(&path).unwrap());

    let connection = direct_connection(&path);
    assert_eq!(
        connection
            .query_row(
                "SELECT count(*) FROM weavelit_configuration \
                 WHERE component = 'totp' AND setting_key = 'mfa-module.enabled'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        0
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT setting_value FROM weavelit_configuration \
                 WHERE component = 'mfa.totp' AND setting_key = 'enabled'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap(),
        "true"
    );
}

#[test]
fn generation_snapshots_settings_and_memberships_are_immutable() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);
    let configuration_id = [0x62_u8; 16];
    let version = 1_u64.to_be_bytes();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation \
             (configuration_id, generation_version, module, name, enabled) \
             VALUES (?1, ?2, 'log-sqlite', 'immutable', 1)",
            params![configuration_id.as_slice(), version.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation_setting \
             (configuration_id, generation_version, setting_key, setting_value) \
             VALUES (?1, ?2, 'retention', '30d')",
            params![configuration_id.as_slice(), version.as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation_log_type \
             (configuration_id, generation_version, log_type) VALUES (?1, ?2, 'audit')",
            params![configuration_id.as_slice(), version.as_slice()],
        )
        .unwrap();

    for statement in [
        "UPDATE weavelit_log_configuration_generation SET enabled = 0",
        "DELETE FROM weavelit_log_configuration_generation",
        "UPDATE weavelit_log_configuration_generation_setting SET setting_value = 'changed'",
        "DELETE FROM weavelit_log_configuration_generation_setting",
        "UPDATE weavelit_log_configuration_generation_log_type SET log_type = 'system'",
        "DELETE FROM weavelit_log_configuration_generation_log_type",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "for {statement}"
        );
    }
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
            (sequence_number, identifier, checksum) VALUES (14, '0014_unknown', ?1)",
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
    assert_eq!(ledger_rows(&direct_connection(&path)).len(), 12);
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

#[test]
fn audit_terminal_recovery_check_constraints_match_public_bounds() {
    // Recovery bounds consistency hardening: verify SQLite migration 0008 CHECK
    // constraint numeric literals match the public shared bounds constants
    // and enforce correct bounds on actual data insertion. This test runs
    // the actual migration and confirms both the SQL constants and runtime
    // enforcement are coherent across Log, Database, and SQLite contracts.

    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    bootstrap(&path);
    let connection = direct_connection(&path);

    // Extract numeric bounds from migration 0008 SQL to verify they match constants.
    let migration_sql = std::str::from_utf8(include_bytes!(
        "../migrations/0008_add_audit_terminal_recovery.sql"
    ))
    .expect("migration SQL is valid UTF-8");

    const EXPECTED_PROJECTION_MAX: usize =
        weavelit_server_database::MAX_AUDIT_TERMINAL_OBLIGATION_BYTES;
    const EXPECTED_DISPOSITION_MAX: usize =
        weavelit_server_database::MAX_AUDIT_TERMINAL_SUPERSESSION_DISPOSITION_BYTES;

    // Verify projection bound is present in migration SQL
    let projection_bound_str = format!("AND {}", EXPECTED_PROJECTION_MAX);
    assert!(
        migration_sql.contains(&projection_bound_str),
        "Migration 0008 SQL must contain projection bound literal {} in CHECK constraint",
        EXPECTED_PROJECTION_MAX
    );

    // Verify disposition bound is present in migration SQL
    let disposition_bound_str = format!("AND {}", EXPECTED_DISPOSITION_MAX);
    assert!(
        migration_sql.contains(&disposition_bound_str),
        "Migration 0008 SQL must contain disposition bound literal {} in CHECK constraint",
        EXPECTED_DISPOSITION_MAX
    );

    // Behavioral test: exactly MAX bytes should be accepted for projection
    let max_projection = vec![0u8; EXPECTED_PROJECTION_MAX];
    let result = connection.execute(
        "INSERT INTO weavelit_audit_terminal_obligation \
         (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
         VALUES (?, ?, ?, ?, 0)",
        params![vec![1u8; 16], max_projection, vec![2u8; 16], vec![3u8; 8],],
    );
    assert!(
        result.is_ok(),
        "INSERT with exactly MAX projection bytes must succeed"
    );

    // Behavioral test: MAX+1 bytes should be rejected for projection
    let oversized_projection = vec![0u8; EXPECTED_PROJECTION_MAX + 1];
    let result = connection.execute(
        "INSERT INTO weavelit_audit_terminal_obligation \
         (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
         VALUES (?, ?, ?, ?, 0)",
        params![
            vec![11u8; 16],
            oversized_projection,
            vec![12u8; 16],
            vec![13u8; 8],
        ],
    );
    assert!(
        result.is_err(),
        "INSERT with MAX+1 projection bytes must fail CHECK constraint"
    );

    // Behavioral test: exactly MAX bytes should be accepted for disposition
    // First insert valid obligations to reference in supersession
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?, ?, ?, ?, 0)",
            params![
                vec![20u8; 16],
                vec![5u8; 128],
                vec![21u8; 16],
                vec![22u8; 8]
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?, ?, ?, ?, 0)",
            params![
                vec![25u8; 16],
                vec![6u8; 128],
                vec![26u8; 16],
                vec![27u8; 8]
            ],
        )
        .unwrap();

    let max_disposition = vec![0u8; EXPECTED_DISPOSITION_MAX];
    let result = connection.execute(
        "INSERT INTO weavelit_audit_terminal_supersession \
         (original_record_identifier, disposition, original_binding_identifier, \
          original_binding_version, replacement_record_identifier, \
          replacement_binding_identifier, replacement_binding_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            vec![20u8; 16],
            max_disposition,
            vec![23u8; 16],
            vec![24u8; 8],
            vec![25u8; 16],
            vec![28u8; 16],
            vec![29u8; 8],
        ],
    );
    assert!(
        result.is_ok(),
        "INSERT with exactly MAX disposition bytes must succeed"
    );

    // Behavioral test: MAX+1 bytes should be rejected for disposition
    // Insert more valid obligations for the oversized disposition test
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?, ?, ?, ?, 0)",
            params![
                vec![30u8; 16],
                vec![7u8; 128],
                vec![31u8; 16],
                vec![32u8; 8]
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?, ?, ?, ?, 0)",
            params![
                vec![35u8; 16],
                vec![8u8; 128],
                vec![36u8; 16],
                vec![37u8; 8]
            ],
        )
        .unwrap();

    let oversized_disposition = vec![0u8; EXPECTED_DISPOSITION_MAX + 1];
    let result = connection.execute(
        "INSERT INTO weavelit_audit_terminal_supersession \
         (original_record_identifier, disposition, original_binding_identifier, \
          original_binding_version, replacement_record_identifier, \
          replacement_binding_identifier, replacement_binding_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            vec![30u8; 16],
            oversized_disposition,
            vec![33u8; 16],
            vec![34u8; 8],
            vec![35u8; 16],
            vec![38u8; 16],
            vec![39u8; 8],
        ],
    );
    assert!(
        result.is_err(),
        "INSERT with MAX+1 disposition bytes must fail CHECK constraint"
    );
}
