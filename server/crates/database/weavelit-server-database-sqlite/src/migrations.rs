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
    Migration {
        sequence: 4,
        identifier: "0004_create_session_store",
        sql: include_str!("../migrations/0004_create_session_store.sql"),
    },
    Migration {
        sequence: 5,
        identifier: "0005_add_mfa_policy_and_replay_watermark",
        sql: include_str!("../migrations/0005_add_mfa_policy_and_replay_watermark.sql"),
    },
    Migration {
        sequence: 6,
        identifier: "0006_add_lifecycle_reconciliation",
        sql: include_str!("../migrations/0006_add_lifecycle_reconciliation.sql"),
    },
    Migration {
        sequence: 7,
        identifier: "0007_add_audit_references",
        sql: include_str!("../migrations/0007_add_audit_references.sql"),
    },
    Migration {
        sequence: 8,
        identifier: "0008_add_audit_terminal_recovery",
        sql: include_str!("../migrations/0008_add_audit_terminal_recovery.sql"),
    },
    Migration {
        sequence: 9,
        identifier: "0009_add_log_configuration_generations",
        sql: include_str!("../migrations/0009_add_log_configuration_generations.sql"),
    },
    Migration {
        sequence: 10,
        identifier: "0010_migrate_totp_component_enablement",
        sql: include_str!("../migrations/0010_migrate_totp_component_enablement.sql"),
    },
    Migration {
        sequence: 11,
        identifier: "0011_add_log_configuration_audit_references",
        sql: include_str!("../migrations/0011_add_log_configuration_audit_references.sql"),
    },
    Migration {
        sequence: 12,
        identifier: "0012_add_account_public_identities",
        sql: include_str!("../migrations/0012_add_account_public_identities.sql"),
    },
    Migration {
        sequence: 13,
        identifier: "0013_add_account_credential_state",
        sql: include_str!("../migrations/0013_add_account_credential_state.sql"),
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
    fn failed_audit_terminal_recovery_migration_rolls_back_schema_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0008_add_audit_terminal_recovery.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..7]).unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[7].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
        );
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_audit_terminal_obligation"
        ));
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_audit_terminal_supersession"
        ));
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            7
        );
    }

    #[test]
    fn failed_log_configuration_generation_backfill_rolls_back_schema_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0009_add_log_configuration_generations.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..8]).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_module_configuration \
                 (configuration_id, module, name, enabled) \
                 VALUES (?1, 'log-sqlite', 'existing', 1)",
                [[0x61_u8; 16].as_slice()],
            )
            .unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[8].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
        );
        for table in [
            "weavelit_log_configuration_generation",
            "weavelit_log_configuration_generation_setting",
            "weavelit_log_configuration_generation_log_type",
            "weavelit_log_configuration_current_generation",
        ] {
            assert!(!table_exists_for_test(&connection, table));
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            8
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT name FROM weavelit_log_module_configuration",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "existing"
        );
    }

    #[test]
    fn failed_totp_enablement_migration_rolls_back_data_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0010_migrate_totp_component_enablement.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..9]).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_lifecycle_state \
                 (singleton, deployment_identifier, state, workflow_kind, checkpoint_metadata) \
                 VALUES (1, ?1, 'initialized', NULL, NULL)",
                [[0x41_u8; 16].as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_configuration \
                 (component, setting_key, setting_value) VALUES ('mfa.totp', 'enabled', 'true')",
                [],
            )
            .unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[9].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
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
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            9
        );
    }

    #[test]
    fn failed_log_configuration_audit_reference_backfill_rolls_back_schema_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0011_add_log_configuration_audit_references.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..10]).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_module_configuration \
                 (configuration_id, module, name, enabled) \
                 VALUES (?1, 'log-sqlite', 'existing', 1)",
                [[0x61_u8; 16].as_slice()],
            )
            .unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[10].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
        );
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_log_configuration_audit_reference"
        ));
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            10
        );
    }

    #[test]
    fn failed_account_public_identity_migration_rolls_back_data_schema_and_ledger() {
        let failures = [
            (
                "zero public identifier",
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (x'22222222222222222222222222222222', 'second', NULL, 1, 0); \
                 INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) \
                 VALUES (x'22222222222222222222222222222222', zeroblob(16));",
            ),
            (
                "public identifier collision",
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (x'22222222222222222222222222222222', 'second', NULL, 1, 0); \
                 INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) \
                 SELECT x'22222222222222222222222222222222', public_identifier \
                 FROM weavelit_account_public_identity LIMIT 1;",
            ),
            (
                "orphan account",
                "INSERT INTO weavelit_account_public_identity \
                 (account_id, public_identifier) \
                 VALUES (x'33333333333333333333333333333333', \
                         x'44444444444444444444444444444444');",
            ),
            (
                "ledger insertion",
                "CREATE TRIGGER reject_0012_ledger \
                 BEFORE INSERT ON weavelit_migration_ledger \
                 WHEN NEW.sequence_number = 12 \
                 BEGIN SELECT RAISE(ABORT, 'reject ledger insertion'); END;",
            ),
        ];

        for (failure, suffix) in failures {
            let mut connection = Connection::open_in_memory().unwrap();
            apply_migrations(&mut connection, &MIGRATIONS[..11]).unwrap();
            let account_id = [0x11_u8; 16];
            connection
                .execute(
                    "INSERT INTO weavelit_account \
                     (account_id, username, display_name, active, mfa_required) \
                     VALUES (?1, 'existing', 'Existing Account', 1, 0)",
                    [account_id.as_slice()],
                )
                .unwrap();
            let failing_sql = Box::leak(
                format!(
                    "{}{}",
                    include_str!("../migrations/0012_add_account_public_identities.sql"),
                    suffix
                )
                .into_boxed_str(),
            );
            let mut migrations = MIGRATIONS.to_vec();
            migrations[11].sql = failing_sql;

            assert_eq!(
                apply_migrations(&mut connection, &migrations),
                Err(DatabaseError::IntegrityFailure),
                "{failure} must fail closed"
            );
            assert!(!table_exists_for_test(
                &connection,
                "weavelit_account_public_identity"
            ));
            assert_eq!(
                connection
                    .query_row(
                        "SELECT count(*) FROM weavelit_migration_ledger",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .unwrap(),
                11
            );
            assert_eq!(
                connection
                    .query_row(
                        "SELECT username, display_name, active, mfa_required \
                         FROM weavelit_account WHERE account_id = ?1",
                        [account_id.as_slice()],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                                row.get::<_, i64>(3)?,
                            ))
                        },
                    )
                    .unwrap(),
                ("existing".to_owned(), "Existing Account".to_owned(), 1, 0)
            );
        }
    }

    #[test]
    fn failed_account_credential_state_migration_rolls_back_columns_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0013_add_account_credential_state.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..12]).unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, 'existing', NULL, 1, 0)",
                [[0x11_u8; 16].as_slice()],
            )
            .unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[12].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
        );
        let columns = connection
            .prepare("SELECT name FROM pragma_table_info('weavelit_account') ORDER BY cid")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(!columns.iter().any(|name| name == "credential_revision"));
        assert!(!columns.iter().any(|name| name == "must_change_password"));
        assert!(
            !columns
                .iter()
                .any(|name| name == "temporary_credential_expires_at_milliseconds")
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            12
        );
        assert_eq!(
            connection
                .query_row("SELECT username FROM weavelit_account", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "existing"
        );
    }

    #[test]
    fn audit_reference_migration_backfills_independent_values_without_changing_entities() {
        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..6]).unwrap();
        let account_id = [0x11_u8; 16];
        let group_id = [0x22_u8; 16];
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, '管理者-équipe', '運用担当', 1, 0)",
                [account_id.as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_group (group_id, name, description) \
                 VALUES (?1, '運用-équipe', 'broad Unicode survives')",
                [group_id.as_slice()],
            )
            .unwrap();

        apply_migrations(&mut connection, MIGRATIONS).unwrap();

        let rows = connection
            .prepare(
                "SELECT audit_reference, 'account' AS entity_kind, account_id AS entity_id \
                 FROM weavelit_account_audit_reference \
                 UNION ALL \
                 SELECT audit_reference, 'group', group_id \
                 FROM weavelit_group_audit_reference \
                 ORDER BY entity_kind",
            )
            .unwrap()
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(rows.len(), 2);
        assert_ne!(rows[0].0, rows[1].0);
        for (reference, _, _) in &rows {
            assert_eq!(reference.len(), 35);
            assert!(reference.starts_with("ar-"));
            assert!(
                reference[3..]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            );
            assert_ne!(reference, "ar-00000000000000000000000000000000");
            assert_ne!(reference, &format!("ar-{}", hex(&account_id)));
            assert_ne!(reference, &format!("ar-{}", hex(&group_id)));
            assert!(!reference.contains("équipe"));
            assert!(!reference.contains("管理者"));
        }
        assert_eq!(rows[0].1, "account");
        assert_eq!(rows[0].2, account_id);
        assert_eq!(rows[1].1, "group");
        assert_eq!(rows[1].2, group_id);
        assert_eq!(
            connection
                .query_row(
                    "SELECT username FROM weavelit_account WHERE account_id = ?1",
                    [account_id.as_slice()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "管理者-équipe"
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT name FROM weavelit_group WHERE group_id = ?1",
                    [group_id.as_slice()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
            "運用-équipe"
        );
    }

    #[test]
    fn failed_audit_reference_backfill_rolls_back_schema_data_and_ledger() {
        const FAILING_MIGRATION: &str = concat!(
            include_str!("../migrations/0007_add_audit_references.sql"),
            "INSERT INTO missing_table VALUES (1);"
        );

        let mut connection = Connection::open_in_memory().unwrap();
        apply_migrations(&mut connection, &MIGRATIONS[..6]).unwrap();
        let account_id = [0x33_u8; 16];
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, 'rollback-user', NULL, 1, 0)",
                [account_id.as_slice()],
            )
            .unwrap();
        let mut migrations = MIGRATIONS.to_vec();
        migrations[6].sql = FAILING_MIGRATION;

        assert_eq!(
            apply_migrations(&mut connection, &migrations),
            Err(DatabaseError::IntegrityFailure)
        );
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_account_audit_reference"
        ));
        assert!(!table_exists_for_test(
            &connection,
            "weavelit_group_audit_reference"
        ));
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM weavelit_migration_ledger",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            6
        );
        assert_eq!(
            connection
                .query_row("SELECT username FROM weavelit_account", [], |row| {
                    row.get::<_, String>(0)
                })
                .unwrap(),
            "rollback-user"
        );
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

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
