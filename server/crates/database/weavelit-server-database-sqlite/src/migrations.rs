use rusqlite::{Connection, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use weavelit_server_database::DatabaseError;

use crate::error::{ErrorContext, map_sqlite_error};

const LEDGER_TABLE: &str = "weavelit_migration_ledger";
const APPLICATION_TABLE_PATTERN: &str = "weavelit_*";

#[derive(Clone, Copy)]
struct Migration {
    sequence: i64,
    identifier: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        sequence: 1,
        identifier: "0001_create_migration_ledger",
        sql: include_str!("../migrations/0001_create_migration_ledger.sql"),
    },
    Migration {
        sequence: 2,
        identifier: "0002_create_lifecycle_state",
        sql: include_str!("../migrations/0002_create_lifecycle_state.sql"),
    },
    Migration {
        sequence: 3,
        identifier: "0003_create_application_state",
        sql: include_str!("../migrations/0003_create_application_state.sql"),
    },
];

struct AppliedMigration {
    sequence: i64,
    identifier: String,
    checksum: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
struct SchemaObject {
    object_type: String,
    name: String,
    table_name: String,
    sql: Option<String>,
}

pub(super) fn apply_pending(connection: &mut Connection) -> Result<(), DatabaseError> {
    apply_migrations(connection, MIGRATIONS)
}

fn apply_migrations(
    connection: &mut Connection,
    migrations: &[Migration],
) -> Result<(), DatabaseError> {
    validate_registry(migrations)?;

    loop {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
        let ledger_exists = table_exists(&transaction, LEDGER_TABLE)?;

        let applied = if ledger_exists {
            let applied = load_applied(&transaction)?;
            if applied.is_empty() {
                return Err(DatabaseError::IntegrityFailure);
            }
            validate_applied(&applied, migrations)?;
            applied
        } else {
            if application_table_exists(&transaction)? {
                return Err(DatabaseError::IntegrityFailure);
            }
            Vec::new()
        };

        validate_schema(&transaction, &migrations[..applied.len()])?;

        let Some(next_migration) = migrations.get(applied.len()) else {
            transaction
                .commit()
                .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
            return Ok(());
        };

        transaction
            .execute_batch(next_migration.sql)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
        let checksum = checksum(next_migration.sql.as_bytes());
        transaction
            .execute(
                "INSERT INTO weavelit_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (?1, ?2, ?3)",
                params![
                    next_migration.sequence,
                    next_migration.identifier,
                    checksum.as_slice()
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
    }
}

fn validate_schema(
    transaction: &Transaction<'_>,
    migrations: &[Migration],
) -> Result<(), DatabaseError> {
    let actual = load_application_schema(transaction)?;
    let expected_connection = Connection::open_in_memory()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
    for migration in migrations {
        expected_connection
            .execute_batch(migration.sql)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
    }
    let expected = load_application_schema(&expected_connection)?;

    if actual != expected {
        return Err(DatabaseError::IntegrityFailure);
    }

    Ok(())
}

fn load_application_schema(connection: &Connection) -> Result<Vec<SchemaObject>, DatabaseError> {
    let mut statement = connection
        .prepare(
            "SELECT type, name, tbl_name, sql FROM sqlite_schema \
             WHERE name GLOB 'weavelit_*' OR tbl_name GLOB 'weavelit_*' \
             ORDER BY type, name, tbl_name",
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
    let rows = statement
        .query_map([], |row| {
            Ok(SchemaObject {
                object_type: row.get(0)?,
                name: row.get(1)?,
                table_name: row.get(2)?,
                sql: row.get(3)?,
            })
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))
}

fn validate_registry(migrations: &[Migration]) -> Result<(), DatabaseError> {
    if migrations.is_empty() {
        return Err(DatabaseError::IntegrityFailure);
    }

    for (index, migration) in migrations.iter().enumerate() {
        let expected_sequence =
            i64::try_from(index + 1).map_err(|_| DatabaseError::IntegrityFailure)?;
        let expected_prefix = format!("{expected_sequence:04}_");
        if migration.sequence != expected_sequence
            || !migration.identifier.starts_with(&expected_prefix)
            || migration.sql.is_empty()
        {
            return Err(DatabaseError::IntegrityFailure);
        }
    }

    Ok(())
}

fn table_exists(transaction: &Transaction<'_>, table_name: &str) -> Result<bool, DatabaseError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))
}

fn application_table_exists(transaction: &Transaction<'_>) -> Result<bool, DatabaseError> {
    transaction
        .query_row(
            "SELECT EXISTS(\
                 SELECT 1 FROM sqlite_schema \
                 WHERE type = 'table' AND name GLOB ?1\
             )",
            [APPLICATION_TABLE_PATTERN],
            |row| row.get(0),
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))
}

fn load_applied(transaction: &Transaction<'_>) -> Result<Vec<AppliedMigration>, DatabaseError> {
    let mut statement = transaction
        .prepare(
            "SELECT sequence_number, identifier, checksum \
             FROM weavelit_migration_ledger ORDER BY sequence_number ASC",
        )
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;
    let rows = statement
        .query_map([], |row| {
            Ok(AppliedMigration {
                sequence: row.get(0)?,
                identifier: row.get(1)?,
                checksum: row.get(2)?,
            })
        })
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Migration))
}

fn validate_applied(
    applied: &[AppliedMigration],
    migrations: &[Migration],
) -> Result<(), DatabaseError> {
    if applied.len() > migrations.len() {
        return Err(DatabaseError::IntegrityFailure);
    }

    for (applied_migration, expected_migration) in applied.iter().zip(migrations) {
        if applied_migration.sequence != expected_migration.sequence
            || applied_migration.identifier != expected_migration.identifier
            || applied_migration.checksum != checksum(expected_migration.sql.as_bytes())
        {
            return Err(DatabaseError::IntegrityFailure);
        }
    }

    Ok(())
}

fn checksum(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_sha256_known_vector() {
        assert_eq!(
            checksum(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn production_registry_is_valid() {
        assert_eq!(validate_registry(MIGRATIONS), Ok(()));
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_ledger_entry() {
        let mut connection = Connection::open_in_memory().unwrap();
        let migrations = [
            MIGRATIONS[0],
            Migration {
                sequence: 2,
                identifier: "0002_create_broken_probe",
                sql: "CREATE TABLE rollback_probe (value INTEGER); \
                      INSERT INTO missing_table VALUES (1);",
            },
        ];

        let error = apply_migrations(&mut connection, &migrations).unwrap_err();

        assert_eq!(error, DatabaseError::IntegrityFailure);
        assert!(table_exists_for_test(&connection, LEDGER_TABLE));
        assert!(!table_exists_for_test(&connection, "rollback_probe"));
        let ledger_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_count, 1);
    }

    #[test]
    fn tampered_applied_prefix_is_rejected_before_pending_migration() {
        let mut connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(MIGRATIONS[0].sql).unwrap();
        let checksum = checksum(MIGRATIONS[0].sql.as_bytes());
        connection
            .execute(
                "INSERT INTO weavelit_migration_ledger \
                 (sequence_number, identifier, checksum) VALUES (1, ?1, ?2)",
                params![MIGRATIONS[0].identifier, checksum.as_slice()],
            )
            .unwrap();
        connection
            .execute_batch("DROP TRIGGER weavelit_migration_ledger_reject_update;")
            .unwrap();

        let error = apply_migrations(&mut connection, MIGRATIONS).unwrap_err();

        assert_eq!(error, DatabaseError::IntegrityFailure);
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_lifecycle_state"
        ));
        let ledger_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM weavelit_migration_ledger",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(ledger_count, 1);
    }

    fn table_exists_for_test(connection: &Connection, table_name: &str) -> bool {
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = ?1)",
                [table_name],
                |row| row.get(0),
            )
            .unwrap()
    }
}
