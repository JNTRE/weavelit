use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore, AuditTerminalReplayBatchSize,
    ConfigurationKey, ConfigurationValue, LogAssignment, LogConfigurationAuditTerminalWrites,
    LogConfigurationGenerationPersistence, LogConfigurationGenerationStore,
    LogConfigurationMutationOutcome, LogConfigurationMutationPersistence,
    LogConfigurationMutationRequest, LogConfigurationMutationStore, LogConfigurationPreparation,
    LogConfigurationVersion, LogModuleSetting, LogType, StateIdentifier,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

struct Surface {
    _directory: TempDir,
    path: PathBuf,
    database: SqliteDatabase,
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn setting(key: &str, value: &str) -> LogModuleSetting {
    LogModuleSetting {
        key: ConfigurationKey::new(key).unwrap(),
        value: ConfigurationValue::new(value).unwrap(),
    }
}

fn surface() -> Surface {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db");
    let database = SqliteDatabase::open(&path).unwrap();
    let connection = Connection::open(&path).unwrap();
    for (byte, name, enabled) in [
        (1, "primary", true),
        (2, "second", true),
        (3, "third", false),
    ] {
        connection
            .execute(
                "INSERT INTO weavelit_log_module_configuration \
                 (configuration_id, module, name, enabled) VALUES (?1, 'sqlite', ?2, ?3)",
                params![
                    identifier(byte).as_bytes().as_slice(),
                    name,
                    i64::from(enabled)
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_generation \
                 (configuration_id, generation_version, module, name, enabled) \
                 VALUES (?1, ?2, 'sqlite', ?3, ?4)",
                params![
                    identifier(byte).as_bytes().as_slice(),
                    1_u64.to_be_bytes().as_slice(),
                    name,
                    i64::from(enabled),
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_current_generation \
                 (configuration_id, generation_version) VALUES (?1, ?2)",
                params![
                    identifier(byte).as_bytes().as_slice(),
                    1_u64.to_be_bytes().as_slice(),
                ],
            )
            .unwrap();
    }
    connection
        .execute(
            "INSERT INTO weavelit_log_module_setting \
             (configuration_id, setting_key, setting_value) VALUES (?1, 'mode', 'old')",
            [identifier(1).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation_setting \
             (configuration_id, generation_version, setting_key, setting_value) \
             VALUES (?1, ?2, 'mode', 'old')",
            params![
                identifier(1).as_bytes().as_slice(),
                1_u64.to_be_bytes().as_slice(),
            ],
        )
        .unwrap();
    for log_type in ["system", "audit"] {
        connection
            .execute(
                "INSERT INTO weavelit_log_assignment (log_type, configuration_id) \
                 VALUES (?1, ?2)",
                params![log_type, identifier(1).as_bytes().as_slice()],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO weavelit_log_configuration_generation_log_type \
                 (configuration_id, generation_version, log_type) VALUES (?1, ?2, ?3)",
                params![
                    identifier(1).as_bytes().as_slice(),
                    1_u64.to_be_bytes().as_slice(),
                    log_type,
                ],
            )
            .unwrap();
    }
    Surface {
        _directory: directory,
        path,
        database,
    }
}

fn persistence() -> (
    LogConfigurationGenerationPersistence,
    LogConfigurationMutationPersistence,
    AuditTerminalRecoveryPersistence,
) {
    let authority = ServerDatabaseAuthority::new();
    (
        LogConfigurationGenerationPersistence::from_server_authority(&authority),
        LogConfigurationMutationPersistence::from_server_authority(&authority),
        AuditTerminalRecoveryPersistence::from_server_authority(&authority),
    )
}

fn terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    byte: u8,
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x44; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [byte; 16],
        vec![byte; 32],
        binding,
    )
    .unwrap()
}

fn prepared(
    database: &mut SqliteDatabase,
    generation: &LogConfigurationGenerationPersistence,
    mutation: &LogConfigurationMutationPersistence,
    request: &LogConfigurationMutationRequest,
) -> weavelit_server_database::PreparedLogConfigurationMutation {
    match database
        .prepare_log_configuration_mutation(generation, mutation, request)
        .unwrap()
    {
        LogConfigurationPreparation::Prepared(prepared) => prepared,
        other => panic!("expected prepared mutation, got {other:?}"),
    }
}

fn pending_identifiers(
    database: &mut SqliteDatabase,
    persistence: &AuditTerminalRecoveryPersistence,
) -> Vec<[u8; 16]> {
    database
        .list_pending_audit_terminal_obligations(
            persistence,
            AuditTerminalReplayBatchSize::new(8).unwrap(),
        )
        .unwrap()
        .into_iter()
        .map(|obligation| *obligation.identifier().as_bytes())
        .collect()
}

fn current_version(path: &Path, configuration: StateIdentifier) -> Vec<u8> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT generation_version FROM weavelit_log_configuration_current_generation \
             WHERE configuration_id = ?1",
            [configuration.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn exact_no_op_including_same_configuration_assignment_writes_nothing() {
    let mut surface = surface();
    let (generation, mutation, audit) = persistence();
    let request = LogConfigurationMutationRequest::new(
        identifier(1),
        Some(true),
        Some(vec![setting("mode", "old")]),
        vec![LogAssignment {
            log_type: LogType::Audit,
            configuration: identifier(1),
        }],
    )
    .unwrap();

    assert!(matches!(
        surface
            .database
            .prepare_log_configuration_mutation(&generation, &mutation, &request)
            .unwrap(),
        LogConfigurationPreparation::Unchanged
    ));
    assert_eq!(
        Connection::open(&surface.path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_configuration_generation",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        3
    );
    assert!(pending_identifiers(&mut surface.database, &audit).is_empty());
}

#[test]
fn settings_change_appends_history_advances_pointer_and_survives_restart() {
    let mut surface = surface();
    let (generation, mutation, audit) = persistence();
    let request = LogConfigurationMutationRequest::new(
        identifier(1),
        None,
        Some(vec![setting("mode", "new")]),
        Vec::new(),
    )
    .unwrap();
    let prepared = prepared(&mut surface.database, &generation, &mutation, &request);
    let applied = terminal(&audit, 0xA1);
    let stale = terminal(&audit, 0xA2);

    assert_eq!(
        surface.database.commit_log_configuration_mutation(
            &prepared,
            &LogConfigurationAuditTerminalWrites::new(&applied, &stale),
        ),
        Ok(LogConfigurationMutationOutcome::Applied {
            generation_count: 1
        })
    );
    assert_eq!(
        current_version(&surface.path, identifier(1)),
        2_u64.to_be_bytes()
    );
    assert_eq!(
        pending_identifiers(&mut surface.database, &audit),
        [[0xA1; 16]]
    );
    drop(surface.database);

    let mut reopened = SqliteDatabase::open(&surface.path).unwrap();
    let old = reopened
        .load_log_configuration_generation(
            &generation,
            generation.key(identifier(1), LogConfigurationVersion::INITIAL),
        )
        .unwrap()
        .unwrap();
    let current = reopened
        .load_current_audit_log_configuration_generation(&generation)
        .unwrap()
        .unwrap();
    assert_eq!(old.settings(), [setting("mode", "old")]);
    assert_eq!(current.key().version().get(), 2);
    assert_eq!(current.settings(), [setting("mode", "new")]);
}

#[test]
fn assignment_moves_version_each_endpoint_once_and_allow_combined_enablement() {
    for assignments in [
        vec![LogAssignment {
            log_type: LogType::Audit,
            configuration: identifier(2),
        }],
        vec![
            LogAssignment {
                log_type: LogType::System,
                configuration: identifier(2),
            },
            LogAssignment {
                log_type: LogType::Audit,
                configuration: identifier(2),
            },
        ],
    ] {
        let mut surface = surface();
        let (generation, mutation, audit) = persistence();
        let request =
            LogConfigurationMutationRequest::new(identifier(2), Some(true), None, assignments)
                .unwrap();
        let prepared = prepared(&mut surface.database, &generation, &mutation, &request);
        assert_eq!(prepared.entries().len(), 2);
        let applied = terminal(&audit, 0xB1);
        let stale = terminal(&audit, 0xB2);
        assert_eq!(
            surface.database.commit_log_configuration_mutation(
                &prepared,
                &LogConfigurationAuditTerminalWrites::new(&applied, &stale),
            ),
            Ok(LogConfigurationMutationOutcome::Applied {
                generation_count: 2
            })
        );
        assert_eq!(
            current_version(&surface.path, identifier(1)),
            2_u64.to_be_bytes()
        );
        assert_eq!(
            current_version(&surface.path, identifier(2)),
            2_u64.to_be_bytes()
        );
    }
}

#[test]
fn invalid_topology_and_version_exhaustion_fail_before_a_plan_exists() {
    let mut surface = surface();
    let (generation, mutation, _) = persistence();
    let unrelated_move = LogConfigurationMutationRequest::new(
        identifier(3),
        Some(false),
        None,
        vec![LogAssignment {
            log_type: LogType::Audit,
            configuration: identifier(2),
        }],
    )
    .unwrap();
    assert!(matches!(
        surface.database.prepare_log_configuration_mutation(
            &generation,
            &mutation,
            &unrelated_move
        ),
        Ok(LogConfigurationPreparation::Invalid)
    ));

    let disable_assigned =
        LogConfigurationMutationRequest::new(identifier(1), Some(false), None, Vec::new()).unwrap();
    assert!(matches!(
        surface.database.prepare_log_configuration_mutation(
            &generation,
            &mutation,
            &disable_assigned
        ),
        Ok(LogConfigurationPreparation::Invalid)
    ));

    let connection = Connection::open(&surface.path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation \
             (configuration_id, generation_version, module, name, enabled) \
             VALUES (?1, ?2, 'sqlite', 'third', 0)",
            params![
                identifier(3).as_bytes().as_slice(),
                u64::MAX.to_be_bytes().as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE weavelit_log_configuration_current_generation \
             SET generation_version = ?1 WHERE configuration_id = ?2",
            params![
                u64::MAX.to_be_bytes().as_slice(),
                identifier(3).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    let exhausted = LogConfigurationMutationRequest::new(
        identifier(3),
        None,
        Some(vec![setting("mode", "new")]),
        Vec::new(),
    )
    .unwrap();
    assert!(matches!(
        surface
            .database
            .prepare_log_configuration_mutation(&generation, &mutation, &exhausted),
        Ok(LogConfigurationPreparation::VersionExhausted)
    ));
}

#[test]
fn competing_preparations_commit_applied_then_stale_with_one_terminal_each() {
    let mut surface = surface();
    let mut competing = SqliteDatabase::open(&surface.path).unwrap();
    let (generation, mutation, audit) = persistence();
    let first_request = LogConfigurationMutationRequest::new(
        identifier(1),
        None,
        Some(vec![setting("mode", "first")]),
        Vec::new(),
    )
    .unwrap();
    let second_request = LogConfigurationMutationRequest::new(
        identifier(1),
        None,
        Some(vec![setting("mode", "second")]),
        Vec::new(),
    )
    .unwrap();
    let first = prepared(
        &mut surface.database,
        &generation,
        &mutation,
        &first_request,
    );
    let second = prepared(&mut competing, &generation, &mutation, &second_request);
    let first_applied = terminal(&audit, 0xC1);
    let first_stale = terminal(&audit, 0xC2);
    let second_applied = terminal(&audit, 0xC3);
    let second_stale = terminal(&audit, 0xC4);

    assert!(matches!(
        surface.database.commit_log_configuration_mutation(
            &first,
            &LogConfigurationAuditTerminalWrites::new(&first_applied, &first_stale),
        ),
        Ok(LogConfigurationMutationOutcome::Applied { .. })
    ));
    assert_eq!(
        competing.commit_log_configuration_mutation(
            &second,
            &LogConfigurationAuditTerminalWrites::new(&second_applied, &second_stale),
        ),
        Ok(LogConfigurationMutationOutcome::Stale)
    );
    assert_eq!(
        pending_identifiers(&mut competing, &audit),
        [[0xC1; 16], [0xC4; 16]]
    );
    assert_eq!(
        current_version(&surface.path, identifier(1)),
        2_u64.to_be_bytes()
    );
}

#[test]
fn terminal_failure_rolls_back_state_history_assignments_and_pointer() {
    let mut surface = surface();
    let (generation, mutation, audit) = persistence();
    let request = LogConfigurationMutationRequest::new(
        identifier(1),
        None,
        Some(vec![setting("mode", "new")]),
        Vec::new(),
    )
    .unwrap();
    let prepared = prepared(&mut surface.database, &generation, &mutation, &request);
    Connection::open(&surface.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_log_configuration_terminal \
             BEFORE INSERT ON weavelit_audit_terminal_obligation \
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    let applied = terminal(&audit, 0xD1);
    let stale = terminal(&audit, 0xD2);

    assert!(
        surface
            .database
            .commit_log_configuration_mutation(
                &prepared,
                &LogConfigurationAuditTerminalWrites::new(&applied, &stale),
            )
            .is_err()
    );
    assert_eq!(
        current_version(&surface.path, identifier(1)),
        1_u64.to_be_bytes()
    );
    assert_eq!(
        Connection::open(&surface.path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_configuration_generation \
                 WHERE configuration_id = ?1",
                [identifier(1).as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
}
