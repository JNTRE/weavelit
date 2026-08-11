use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    Account, AccountPasswordVerifier, ApplicationDatabase, ApplicationState, ApplicationStateInput,
    COMPONENT_ENABLED_VALUE, CheckpointMetadata, CompletionObligation, ComponentEnablement,
    ComponentKind, ConfigurationEntry, ConfigurationKey, ConfigurationValue, CorrelationIdentifier,
    DatabaseError, DatabaseInspection, DeploymentIdentifier, Group, GroupGrant, GroupGrantRecord,
    GroupMembership, LogAssignment, LogClassification, LogDetail, LogModuleConfiguration,
    LogModuleSetting, LogType, MfaFactor, MfaStore, MfaTimeStep, Name, NewSession,
    PasswordVerifier, ProtectedSecret, ProtectedValue, RecoveryPublicKey, SESSION_DIGEST_LENGTH,
    ServiceConnection, SessionCsrfHash, SessionInstant, SessionStore, SessionTokenHash,
    StateIdentifier, WorkflowCheckpoint, WorkflowKind,
};
use weavelit_server_database_sqlite::SqliteDatabase;

const PROTECTED_SECRET_BYTES: &[u8] = b"protected-component-secret";
const PROTECTED_FACTOR_BYTES: &[u8] = b"protected-factor-data";
const PROTECTED_CREDENTIAL_BYTES: &[u8] = b"protected-provider-credential";
const VERIFIER: &str = "$argon2id$v=19$m=65536,t=3,p=1$c2FsdHNhbHRzYWx0$dmVyaWZpZXI";
const RECOVERY_KEY: &str = "age1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqsm5xurc";
const USERNAME: &str = "first-admin";
const CHECKPOINT_METADATA: &[u8] = b"restore-checkpoint-metadata";
const RECORD_IDENTIFIER_BYTE: u8 = 0xF0;
const SESSION_CLIENT_MODULE: &str = "session-marker-module";

const EXPECTED_TABLES: [&str; 18] = [
    "weavelit_account",
    "weavelit_completion_obligation",
    "weavelit_configuration",
    "weavelit_group",
    "weavelit_group_grant",
    "weavelit_group_membership",
    "weavelit_lifecycle_state",
    "weavelit_log_assignment",
    "weavelit_log_module_configuration",
    "weavelit_log_module_setting",
    "weavelit_migration_ledger",
    "weavelit_mfa_factor",
    "weavelit_mfa_replay_watermark",
    "weavelit_password_verifier",
    "weavelit_protected_secret",
    "weavelit_recovery_public_key",
    "weavelit_service_connection",
    "weavelit_session",
];

/// The complete live session table, which may hold only digests, the owning
/// account and Client Module, and the three lifetime instants.
const EXPECTED_SESSION_COLUMNS: [&str; 7] = [
    "weavelit_session.absolute_expires_at_milliseconds",
    "weavelit_session.account_id",
    "weavelit_session.client_module",
    "weavelit_session.csrf_hash",
    "weavelit_session.issued_at_milliseconds",
    "weavelit_session.last_seen_at_milliseconds",
    "weavelit_session.token_hash",
];

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn deployment(byte: u8) -> DeploymentIdentifier {
    DeploymentIdentifier::from_bytes([byte; 16]).unwrap()
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn name(value: &str) -> Name {
    Name::new(value).unwrap()
}

fn session(token_byte: u8, csrf_byte: u8) -> NewSession {
    NewSession::new(
        SessionTokenHash::from_bytes([token_byte; SESSION_DIGEST_LENGTH]).unwrap(),
        SessionCsrfHash::from_bytes([csrf_byte; SESSION_DIGEST_LENGTH]).unwrap(),
        identifier(1),
        name(SESSION_CLIENT_MODULE),
        SessionInstant::from_unix_milliseconds(1_000).unwrap(),
    )
}

fn session_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM weavelit_session", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn watermark_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM weavelit_mfa_replay_watermark",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn restore_checkpoint(deployment_identifier: DeploymentIdentifier) -> WorkflowCheckpoint {
    WorkflowCheckpoint::new(
        deployment_identifier,
        WorkflowKind::Restore,
        CheckpointMetadata::from_bytes(CHECKPOINT_METADATA).unwrap(),
    )
}

fn obligation(workflow: WorkflowKind) -> CompletionObligation {
    CompletionObligation::new(
        identifier(RECORD_IDENTIFIER_BYTE),
        workflow,
        LogClassification::new("lifecycle.restore").unwrap(),
        CorrelationIdentifier::new("correlation-identifier").unwrap(),
        1_700_000_000_000,
        LogDetail::new("workflow completed").unwrap(),
    )
    .unwrap()
}

fn application_state(workflow: WorkflowKind) -> ApplicationState {
    ApplicationState::new(ApplicationStateInput {
        configuration: vec![
            ConfigurationEntry {
                component: name("mfa.totp"),
                key: ConfigurationKey::new("enabled").unwrap(),
                value: ConfigurationValue::new("false").unwrap(),
            },
            ConfigurationEntry {
                component: name("server"),
                key: ConfigurationKey::new("display-name").unwrap(),
                value: ConfigurationValue::new("Weavelit").unwrap(),
            },
        ],
        protected_secrets: vec![ProtectedSecret {
            component: name("server"),
            key: ConfigurationKey::new("component-secret").unwrap(),
            value: ProtectedValue::new(PROTECTED_SECRET_BYTES).unwrap(),
        }],
        accounts: vec![
            Account {
                identifier: identifier(1),
                username: name(USERNAME),
                display_name: Some(name("First Admin")),
                active: true,
            },
            Account {
                identifier: identifier(2),
                username: name("disabled-user"),
                display_name: None,
                active: false,
            },
        ],
        password_verifiers: vec![AccountPasswordVerifier {
            account: identifier(1),
            verifier: PasswordVerifier::new(VERIFIER).unwrap(),
        }],
        groups: vec![
            Group {
                identifier: identifier(3),
                name: name("Administrators"),
                description: Some(
                    weavelit_server_database::Description::new("Server administration").unwrap(),
                ),
            },
            Group {
                identifier: identifier(7),
                name: name("Ticket Operators"),
                description: None,
            },
        ],
        group_memberships: vec![
            GroupMembership {
                group: identifier(3),
                account: identifier(1),
            },
            GroupMembership {
                group: identifier(7),
                account: identifier(1),
            },
        ],
        group_grants: vec![
            GroupGrantRecord {
                group: identifier(3),
                grant: GroupGrant::ServerAdministration,
            },
            GroupGrantRecord {
                group: identifier(3),
                grant: GroupGrant::ClientModule(name("web-ui")),
            },
            GroupGrantRecord {
                group: identifier(3),
                grant: GroupGrant::ServiceModule(name("zendesk")),
            },
            GroupGrantRecord {
                group: identifier(3),
                grant: GroupGrant::Operation(name("zendesk.ticket.create")),
            },
            // The second Group overlaps on the Client Module, the Service
            // Module, and one Operation, and adds one Operation of its own.
            GroupGrantRecord {
                group: identifier(7),
                grant: GroupGrant::ClientModule(name("web-ui")),
            },
            GroupGrantRecord {
                group: identifier(7),
                grant: GroupGrant::ServiceModule(name("zendesk")),
            },
            GroupGrantRecord {
                group: identifier(7),
                grant: GroupGrant::Operation(name("zendesk.ticket.create")),
            },
            GroupGrantRecord {
                group: identifier(7),
                grant: GroupGrant::Operation(name("zendesk.ticket.comment")),
            },
        ],
        mfa_factors: vec![MfaFactor {
            identifier: identifier(4),
            account: identifier(1),
            module: name("totp"),
            protected_factor_data: ProtectedValue::new(PROTECTED_FACTOR_BYTES).unwrap(),
        }],
        service_connections: vec![ServiceConnection {
            identifier: identifier(5),
            service_module: name("zendesk"),
            name: name("primary"),
            protected_credential: ProtectedValue::new(PROTECTED_CREDENTIAL_BYTES).unwrap(),
        }],
        recovery_public_key: RecoveryPublicKey::new(RECOVERY_KEY).unwrap(),
        log_module_configurations: vec![LogModuleConfiguration {
            identifier: identifier(6),
            module: name("log-sqlite"),
            name: name("local"),
            enabled: true,
            settings: vec![LogModuleSetting {
                key: ConfigurationKey::new("retention").unwrap(),
                value: ConfigurationValue::new("unsupported").unwrap(),
            }],
        }],
        log_assignments: vec![
            LogAssignment {
                log_type: LogType::System,
                configuration: identifier(6),
            },
            LogAssignment {
                log_type: LogType::Audit,
                configuration: identifier(6),
            },
        ],
        completion_obligation: obligation(workflow),
    })
    .unwrap()
}

fn restored_database(path: &Path, deployment_identifier: DeploymentIdentifier) -> SqliteDatabase {
    let mut database = SqliteDatabase::open(path).unwrap();
    let checkpoint = restore_checkpoint(deployment_identifier);
    database.create_checkpoint(&checkpoint).unwrap();
    database
        .complete_checkpoint(&checkpoint, &application_state(WorkflowKind::Restore))
        .unwrap();
    database
}

fn pending_database(path: &Path, deployment_identifier: DeploymentIdentifier) -> SqliteDatabase {
    let mut database = SqliteDatabase::open(path).unwrap();
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    database
}

fn table_names(path: &Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT name FROM sqlite_schema WHERE type = 'table' \
             AND name GLOB 'weavelit_*' ORDER BY name",
        )
        .unwrap();
    let names = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    drop(statement);
    names
}

fn column_names(path: &Path) -> Vec<String> {
    let connection = Connection::open(path).unwrap();
    let mut columns = Vec::new();
    for table in table_names(path) {
        let mut statement = connection
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        let table_columns = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        columns.extend(
            table_columns
                .into_iter()
                .map(|column| format!("{table}.{column}")),
        );
    }
    columns
}

fn state_row_counts(path: &Path) -> Vec<(String, i64)> {
    let connection = Connection::open(path).unwrap();
    table_names(path)
        .into_iter()
        .filter(|table| table != "weavelit_migration_ledger" && table != "weavelit_lifecycle_state")
        .map(|table| {
            let count = connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .unwrap();
            (table, count)
        })
        .collect()
}

fn assert_no_state_rows(path: &Path) {
    for (table, count) in state_row_counts(path) {
        assert_eq!(count, 0, "table {table} must be empty");
    }
}

fn assert_redacted(error: DatabaseError) {
    let message = error.to_string();
    let lower_message = message.to_ascii_lowercase();
    for secret in [
        String::from_utf8_lossy(PROTECTED_SECRET_BYTES).to_string(),
        String::from_utf8_lossy(PROTECTED_FACTOR_BYTES).to_string(),
        String::from_utf8_lossy(PROTECTED_CREDENTIAL_BYTES).to_string(),
        VERIFIER.to_string(),
        RECOVERY_KEY.to_string(),
        USERNAME.to_string(),
        String::from_utf8_lossy(CHECKPOINT_METADATA).to_string(),
    ] {
        assert!(
            !message.contains(&secret),
            "error exposed a protected value"
        );
    }
    assert!(!lower_message.contains("sqlite"));
    assert!(!lower_message.contains("insert"));
    assert!(!lower_message.contains("select"));
    assert!(message.starts_with("application database"));
}

/// Proves the same intent the previous exact-column-name test carried: that
/// sessions, log records, and Log Module credentials are not part of restorable
/// state and cannot ride in a backup. It no longer conflates that intent with
/// "no session table exists", because the specification requires the Server to
/// store live sessions in the Application Database.
#[test]
fn only_the_live_session_table_may_name_session_data_and_no_table_stores_log_records() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    drop(SqliteDatabase::open(&path).unwrap());

    let mut expected = EXPECTED_TABLES
        .iter()
        .map(|table| (*table).to_owned())
        .collect::<Vec<_>>();
    expected.sort();

    assert_eq!(table_names(&path), expected);

    let mut session_columns = column_names(&path)
        .into_iter()
        .filter(|column| column.starts_with("weavelit_session."))
        .collect::<Vec<_>>();
    session_columns.sort();
    assert_eq!(session_columns, EXPECTED_SESSION_COLUMNS);

    for column in column_names(&path) {
        let lower_column = column.to_ascii_lowercase();
        assert!(!lower_column.contains("log_record"));
        if lower_column.starts_with("weavelit_session.") {
            continue;
        }
        assert!(!lower_column.contains("session"));
        assert!(!lower_column.contains("token"));
        assert!(!lower_column.contains("csrf"));
    }
    let log_module_columns = column_names(&path)
        .into_iter()
        .filter(|column| column.starts_with("weavelit_log_module_"))
        .collect::<Vec<_>>();
    for column in &log_module_columns {
        let lower_column = column.to_ascii_lowercase();
        assert!(!lower_column.contains("credential"));
        assert!(!lower_column.contains("secret"));
        assert!(!lower_column.contains("password"));
    }
}

#[test]
fn the_authorization_projection_unions_the_grants_of_every_membership_group() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(20));

    let snapshot = database
        .load_human_authorization(identifier(1))
        .unwrap()
        .expect("a held account must project");

    assert!(snapshot.active());
    // Both Groups grant the Web UI Client Module, the Zendesk Service Module,
    // and the create Operation; only one grants the comment Operation and only
    // one grants the Server Administration Permission. The join reports each
    // distinct grant once.
    assert_eq!(
        snapshot.grants(),
        [
            GroupGrant::ClientModule(name("web-ui")),
            GroupGrant::Operation(name("zendesk.ticket.comment")),
            GroupGrant::Operation(name("zendesk.ticket.create")),
            GroupGrant::ServerAdministration,
            GroupGrant::ServiceModule(name("zendesk")),
        ]
    );
}

#[test]
fn the_authorization_projection_separates_an_absent_account_from_a_grantless_one() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(21));

    let inactive = database
        .load_human_authorization(identifier(2))
        .unwrap()
        .expect("a held account must project even with no membership");
    let unknown = database.load_human_authorization(identifier(9)).unwrap();

    assert!(!inactive.active());
    assert!(inactive.grants().is_empty());
    assert_eq!(unknown, None);
}

#[test]
fn the_authorization_projection_renders_without_revealing_what_it_carries() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(22));

    let snapshot = database
        .load_human_authorization(identifier(1))
        .unwrap()
        .expect("a held account must project");
    let rendered = format!("{snapshot:?}");

    // What the projection carries is asserted structurally by the two tests
    // above. This one pins the separate property that rendering it cannot
    // disclose it, because the snapshot is reachable from request-handling code
    // that logs. The scan is anchored on a control value that must appear, so
    // it cannot pass merely because the rendering redacted everything into
    // nothing or failed to name the type at all.
    assert!(
        rendered.contains("HumanAuthorizationSnapshot"),
        "the rendering names the projection"
    );
    for excluded in [
        String::from_utf8_lossy(PROTECTED_SECRET_BYTES).to_string(),
        String::from_utf8_lossy(PROTECTED_FACTOR_BYTES).to_string(),
        String::from_utf8_lossy(PROTECTED_CREDENTIAL_BYTES).to_string(),
        VERIFIER.to_string(),
        RECOVERY_KEY.to_string(),
        USERNAME.to_string(),
        String::from("Administrators"),
        String::from("Ticket Operators"),
        String::from("log-sqlite"),
        String::from("primary"),
        // The grants the projection genuinely does carry are redacted too, so a
        // later derived `Debug` on a name type cannot start leaking them.
        String::from("web-ui"),
        String::from("zendesk.ticket.create"),
    ] {
        assert!(
            !rendered.contains(&excluded),
            "the projection exposed {excluded}"
        );
    }
}

#[test]
fn every_component_is_enabled_until_an_entry_disables_it() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(23));

    // The seeded state carries component configuration but no enablement
    // entry, so nothing is disabled.
    assert_eq!(
        database.load_component_enablement().unwrap(),
        ComponentEnablement::default()
    );

    disable_component(&path, ComponentKind::ClientModule, "web-ui", "false");
    disable_component(
        &path,
        ComponentKind::Operation,
        "zendesk.ticket.create",
        "false",
    );
    // A value the Server does not recognize disables rather than enables.
    disable_component(&path, ComponentKind::ServiceModule, "zendesk", "yes");
    // The exactly-enabled value leaves the component reachable.
    disable_component(
        &path,
        ComponentKind::MfaModule,
        "totp",
        COMPONENT_ENABLED_VALUE,
    );

    let enablement = database.load_component_enablement().unwrap();

    assert!(!enablement.is_enabled(ComponentKind::ClientModule, &name("web-ui")));
    assert!(!enablement.is_enabled(ComponentKind::Operation, &name("zendesk.ticket.create")));
    assert!(!enablement.is_enabled(ComponentKind::ServiceModule, &name("zendesk")));
    assert!(enablement.is_enabled(ComponentKind::MfaModule, &name("totp")));
    // A Client Module and a Service Module of the same name are separate.
    assert!(enablement.is_enabled(ComponentKind::ServiceModule, &name("web-ui")));
}

#[test]
fn the_enablement_projection_reads_no_other_component_setting() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(24));
    disable_component(&path, ComponentKind::ClientModule, "web-ui", "false");

    let enablement = database.load_component_enablement().unwrap();

    // The projection is asserted through its own accessor rather than through a
    // rendered `Debug`, because `Name` redacts its text: a scan of the rendering
    // would pass against a projection carrying every secret in the deployment.
    let disabled: Vec<(ComponentKind, String)> = enablement
        .disabled()
        .map(|(kind, name)| (kind, name.as_str().to_owned()))
        .collect();

    // The seeded state carries a display name, an MFA Module setting, an
    // account, its verifier, and a recovery key. None of them is a component
    // enablement entry, so exactly one entry is projected and nothing else is
    // reachable through this read at all.
    assert_eq!(
        disabled,
        vec![(ComponentKind::ClientModule, String::from("web-ui"))]
    );
}

/// Writes one component enablement entry directly, as an administrator's
/// enablement change would, without reopening or reloading the database.
fn disable_component(path: &Path, kind: ComponentKind, component: &str, value: &str) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO weavelit_configuration \
             (component, setting_key, setting_value) VALUES (?1, ?2, ?3)",
            rusqlite::params![component, kind.enablement_key(), value],
        )
        .unwrap();
}

#[test]
fn a_restore_clears_every_live_session_inside_the_state_replacement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(12);
    let mut database = pending_database(&path, deployment_identifier);
    database.create(&session(0x21, 0x22)).unwrap();
    database.create(&session(0x23, 0x24)).unwrap();
    assert_eq!(session_count(&path), 2);

    database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap();

    assert_eq!(session_count(&path), 0);
    assert_eq!(
        database
            .load_initialized_state(deployment_identifier)
            .unwrap()
            .state(),
        &application_state(WorkflowKind::Restore)
    );
}

/// A Restore replaces live state as well as restorable state, and a replay
/// watermark is live: it records what a factor did in this deployment, not
/// what the restored aggregate says a factor is. Keeping one across a Restore
/// would judge a code against another history.
#[test]
fn a_restore_clears_every_replay_watermark_inside_the_state_replacement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(13);
    let mut database = pending_database(&path, deployment_identifier);
    let step = MfaTimeStep::from_step(41_152_263).unwrap();
    database.accept_step(identifier(5), step).unwrap();
    database.accept_step(identifier(7), step).unwrap();
    assert_eq!(watermark_count(&path), 2);

    database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap();

    assert_eq!(watermark_count(&path), 0);
    assert_eq!(database.accepted_step(identifier(5)).unwrap(), None);
    assert_eq!(
        database.accept_step(identifier(5), step).unwrap(),
        weavelit_server_database::MfaAcceptance::Accepted
    );
}

#[test]
fn a_rejected_state_replacement_leaves_live_sessions_untouched() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(13);
    let mut database = pending_database(&path, deployment_identifier);
    database.create(&session(0x25, 0x26)).unwrap();

    let error = database
        .complete_checkpoint(
            &WorkflowCheckpoint::new(
                deployment_identifier,
                WorkflowKind::Restore,
                CheckpointMetadata::from_bytes(b"other-metadata".as_slice()).unwrap(),
            ),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap_err();

    assert_eq!(error, DatabaseError::InvalidState);
    assert_eq!(
        session_count(&path),
        1,
        "a replacement that did not commit must not have cleared sessions"
    );
}

#[test]
fn normalized_state_and_backup_content_carry_no_session_data() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(14);
    let mut database = restored_database(&path, deployment_identifier);
    database.create(&session(0x27, 0x28)).unwrap();
    database.create(&session(0x29, 0x2A)).unwrap();

    let loaded = database
        .load_initialized_state(deployment_identifier)
        .unwrap();
    let rendered = format!("{:?}", loaded.state());

    assert_eq!(
        session_count(&path),
        2,
        "the live sessions must still be stored"
    );
    assert_eq!(
        loaded.state(),
        &application_state(WorkflowKind::Restore),
        "stored sessions must not change the normalized aggregate a backup is built from"
    );
    assert!(!rendered.contains(SESSION_CLIENT_MODULE));
    assert!(!rendered.to_ascii_lowercase().contains("session"));
    assert!(!rendered.to_ascii_lowercase().contains("csrf"));
}

#[test]
fn completion_persists_every_state_type_and_reloads_it_across_reopen() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(1);
    let expected = application_state(WorkflowKind::Restore);
    drop(restored_database(&path, deployment_identifier));

    let mut database = SqliteDatabase::open(&path).unwrap();
    let loaded = database
        .load_initialized_state(deployment_identifier)
        .unwrap();

    assert_eq!(loaded.deployment_identifier(), deployment_identifier);
    assert!(!loaded.completion_acknowledged());
    assert_eq!(loaded.state(), &expected);
    assert_eq!(loaded.state().accounts().len(), 2);
    assert_eq!(loaded.state().group_grants().len(), 8);
    assert_eq!(
        loaded.state().password_verifiers()[0].verifier.as_str(),
        VERIFIER
    );
    assert_eq!(
        loaded.state().mfa_factors()[0]
            .protected_factor_data
            .as_bytes(),
        PROTECTED_FACTOR_BYTES
    );
    assert_eq!(
        loaded.state().service_connections()[0]
            .protected_credential
            .as_bytes(),
        PROTECTED_CREDENTIAL_BYTES
    );
    assert_eq!(loaded.state().recovery_public_key().as_str(), RECOVERY_KEY);
    assert_eq!(
        loaded.state().log_module_configurations()[0].settings.len(),
        1
    );
    assert_eq!(loaded.state().log_assignments().len(), 2);
    assert_eq!(
        database.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Initialized {
            deployment_identifier
        }
    );
}

#[test]
fn completion_is_accepted_exactly_once() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(2);
    let mut database = restored_database(&path, deployment_identifier);
    let before = state_row_counts(&path);

    let error = database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap_err();

    assert_eq!(error, DatabaseError::AlreadyInitialized);
    assert_eq!(state_row_counts(&path), before);
    assert_redacted(error);
}

#[test]
fn mismatched_checkpoint_is_rejected_without_writing_state() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(3);
    let mut database = pending_database(&path, deployment_identifier);
    let different_metadata = WorkflowCheckpoint::new(
        deployment_identifier,
        WorkflowKind::Restore,
        CheckpointMetadata::from_bytes(b"other-metadata".as_slice()).unwrap(),
    );
    let different_workflow = WorkflowCheckpoint::new(
        deployment_identifier,
        WorkflowKind::Init,
        CheckpointMetadata::from_bytes(CHECKPOINT_METADATA).unwrap(),
    );

    for (checkpoint, workflow) in [
        (different_metadata, WorkflowKind::Restore),
        (different_workflow, WorkflowKind::Init),
    ] {
        let error = database
            .complete_checkpoint(&checkpoint, &application_state(workflow))
            .unwrap_err();

        assert_eq!(error, DatabaseError::InvalidState);
        assert_no_state_rows(&path);
        assert_redacted(error);
    }
    assert_eq!(
        database.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Pending(restore_checkpoint(deployment_identifier))
    );
}

#[test]
fn deployment_mismatch_is_rejected_for_completion_load_and_acknowledgement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let persisted = deployment(4);
    let other = deployment(5);
    let mut database = pending_database(&path, persisted);

    let completion_error = database
        .complete_checkpoint(
            &restore_checkpoint(other),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap_err();
    let load_error = database.load_initialized_state(other).unwrap_err();
    let acknowledgement_error = database
        .acknowledge_completion(other, identifier(RECORD_IDENTIFIER_BYTE))
        .unwrap_err();

    assert_eq!(completion_error, DatabaseError::DeploymentMismatch);
    assert_eq!(load_error, DatabaseError::DeploymentMismatch);
    assert_eq!(acknowledgement_error, DatabaseError::DeploymentMismatch);
    assert_no_state_rows(&path);
    assert_redacted(completion_error);
}

#[test]
fn completion_requires_a_pending_checkpoint_and_matching_obligation_workflow() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(6);
    let mut database = SqliteDatabase::open(&path).unwrap();

    let uninitialized_error = database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap_err();
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    let obligation_error = database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Init),
        )
        .unwrap_err();

    assert_eq!(uninitialized_error, DatabaseError::InvalidState);
    assert_eq!(obligation_error, DatabaseError::InvalidState);
    assert_no_state_rows(&path);
}

#[test]
fn transaction_failure_rolls_back_every_persisted_state_row() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(7);
    let mut database = pending_database(&path, deployment_identifier);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER test_reject_account_insert \
             BEFORE INSERT ON weavelit_account \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();
    drop(connection);

    let error = database
        .complete_checkpoint(
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
        )
        .unwrap_err();
    drop(database);

    assert_eq!(error, DatabaseError::IntegrityFailure);
    assert_no_state_rows(&path);
    assert_redacted(error);
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TRIGGER test_reject_account_insert;")
        .unwrap();
    let mut reopened = SqliteDatabase::open(&path).unwrap();
    assert_eq!(
        reopened.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Pending(restore_checkpoint(deployment_identifier))
    );
    assert_eq!(
        reopened.load_initialized_state(deployment_identifier),
        Err(DatabaseError::NotInitialized)
    );
}

#[test]
fn loading_uninitialized_or_pending_state_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(8);
    let mut database = SqliteDatabase::open(&path).unwrap();

    let uninitialized = database.load_initialized_state(deployment_identifier);
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    let pending = database.load_initialized_state(deployment_identifier);

    assert_eq!(uninitialized, Err(DatabaseError::NotInitialized));
    assert_eq!(pending, Err(DatabaseError::NotInitialized));
}

#[test]
fn malformed_persisted_state_fails_integrity_validation() {
    let mutations = [
        "DELETE FROM weavelit_recovery_public_key",
        "DELETE FROM weavelit_log_assignment WHERE log_type = 'audit'",
        "DELETE FROM weavelit_completion_obligation",
        "UPDATE weavelit_log_module_configuration SET enabled = 0",
    ];

    for mutation in mutations {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let deployment_identifier = deployment(9);
        drop(restored_database(&path, deployment_identifier));
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);

        let error = SqliteDatabase::open(&path)
            .unwrap()
            .load_initialized_state(deployment_identifier)
            .unwrap_err();

        assert_eq!(error, DatabaseError::IntegrityFailure, "for {mutation}");
        assert_redacted(error);
    }
}

#[test]
fn completion_obligation_is_acknowledged_exactly_once() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(10);
    let mut database = restored_database(&path, deployment_identifier);
    let record_identifier = identifier(RECORD_IDENTIFIER_BYTE);

    let unknown_record = database.acknowledge_completion(deployment_identifier, identifier(0x11));
    let first = database.acknowledge_completion(deployment_identifier, record_identifier);
    let second = database.acknowledge_completion(deployment_identifier, record_identifier);
    let loaded = database
        .load_initialized_state(deployment_identifier)
        .unwrap();

    assert_eq!(unknown_record, Err(DatabaseError::InvalidState));
    assert_eq!(first, Ok(()));
    assert_eq!(second, Err(DatabaseError::InvalidState));
    assert!(loaded.completion_acknowledged());
    assert_eq!(
        loaded.state().completion_obligation(),
        &obligation(WorkflowKind::Restore)
    );
}

#[test]
fn acknowledgement_requires_initialized_state() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(11);
    let mut database = SqliteDatabase::open(&path).unwrap();

    let uninitialized =
        database.acknowledge_completion(deployment_identifier, identifier(RECORD_IDENTIFIER_BYTE));
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    let pending =
        database.acknowledge_completion(deployment_identifier, identifier(RECORD_IDENTIFIER_BYTE));

    assert_eq!(uninitialized, Err(DatabaseError::NotInitialized));
    assert_eq!(pending, Err(DatabaseError::NotInitialized));
}
