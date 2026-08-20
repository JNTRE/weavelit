use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    Account, AccountAdministrationStore, AccountAuditReference, AccountCreateMutation,
    AccountCreateOutcome, AccountCredentialAuditTerminalWrites, AccountCredentialIssuanceFactor,
    AccountCredentialIssuanceRecheck, AccountCredentialWriterStore, AccountPasswordResetMutation,
    AccountPasswordResetOutcome, AccountPasswordVerifier, AccountPublicIdentifier,
    AccountPublicIdentifierPersistence, AccountPublicIdentity, ApplicationDatabase,
    ApplicationState, ApplicationStateInput, AuditReferenceIdentifier, AuditReferencePersistence,
    AuditTerminalRecoveryPersistence, COMPONENT_ENABLED_VALUE, CheckpointMetadata,
    CompletionObligation, ComponentEnablement, ComponentKind, ConfigurationEntry, ConfigurationKey,
    ConfigurationValue, CorrelationIdentifier, CredentialRevision, DatabaseError,
    DatabaseInspection, DeploymentIdentifier, Group, GroupAuditReference, GroupGrant,
    GroupGrantRecord, GroupMembership, LogAssignment, LogClassification,
    LogConfigurationAuditReference, LogConfigurationGenerationPersistence, LogConfigurationVersion,
    LogDetail, LogModuleConfiguration, LogModuleSetting, LogType, MfaFactor, MfaModuleTarget,
    MfaStore, MfaTimeStep, Name, NewSession, PasswordVerifier, ProtectedSecret, ProtectedValue,
    ReconciliationDigest, ReconciliationStore, RecoveryPublicKey, SESSION_DIGEST_LENGTH,
    ServiceConnection, SessionCsrfHash, SessionInstant, SessionStore, SessionTokenHash,
    StateIdentifier, StoredAuditDestinationBinding, TemporaryCredentialExpiration,
    ValidatedAuditTerminalObligationWrite, WorkflowCheckpoint, WorkflowKind,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
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

const EXPECTED_TABLES: [&str; 29] = [
    "weavelit_account",
    "weavelit_account_audit_reference",
    "weavelit_account_public_identity",
    "weavelit_audit_terminal_obligation",
    "weavelit_audit_terminal_supersession",
    "weavelit_completion_obligation",
    "weavelit_configuration",
    "weavelit_group",
    "weavelit_group_audit_reference",
    "weavelit_group_grant",
    "weavelit_group_membership",
    "weavelit_lifecycle_state",
    "weavelit_lifecycle_reconciliation",
    "weavelit_log_configuration_current_generation",
    "weavelit_log_configuration_generation",
    "weavelit_log_configuration_generation_log_type",
    "weavelit_log_configuration_generation_setting",
    "weavelit_log_assignment",
    "weavelit_log_configuration_audit_reference",
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

fn account_public_identifier(byte: u8) -> AccountPublicIdentifier {
    account_public_identifier_persistence()
        .decode([byte; 16])
        .unwrap()
}

fn account_public_identifier_persistence() -> AccountPublicIdentifierPersistence {
    static PERSISTENCE: OnceLock<AccountPublicIdentifierPersistence> = OnceLock::new();

    *PERSISTENCE.get_or_init(|| {
        AccountPublicIdentifierPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    })
}

fn audit_reference(byte: u8) -> AuditReferenceIdentifier {
    audit_reference_persistence()
        .decode(&format!("ar-{}", format!("{byte:02x}").repeat(16)))
        .unwrap()
}

fn audit_reference_persistence() -> AuditReferencePersistence {
    static PERSISTENCE: OnceLock<AuditReferencePersistence> = OnceLock::new();

    *PERSISTENCE.get_or_init(|| {
        AuditReferencePersistence::from_server_authority(&ServerDatabaseAuthority::new())
    })
}

fn log_configuration_generation_persistence() -> &'static LogConfigurationGenerationPersistence {
    static PERSISTENCE: OnceLock<LogConfigurationGenerationPersistence> = OnceLock::new();

    PERSISTENCE.get_or_init(|| {
        LogConfigurationGenerationPersistence::from_server_authority(&ServerDatabaseAuthority::new())
    })
}

fn reconciliation_digest(byte: u8) -> ReconciliationDigest {
    ReconciliationDigest::from_bytes([byte; 32])
}

fn name(value: &str) -> Name {
    Name::new(value).unwrap()
}

fn session(token_byte: u8, csrf_byte: u8) -> NewSession {
    NewSession::new(
        SessionTokenHash::from_bytes([token_byte; SESSION_DIGEST_LENGTH]).unwrap(),
        SessionCsrfHash::from_bytes([csrf_byte; SESSION_DIGEST_LENGTH]).unwrap(),
        identifier(1),
        CredentialRevision::from_value(u64::MAX).unwrap(),
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

fn insert_session_issuance_account(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required, credential_revision) \
             VALUES (?1, 'session-owner', NULL, 1, 0, ?2)",
            rusqlite::params![
                identifier(1).as_bytes().as_slice(),
                u64::MAX.to_be_bytes().as_slice(),
            ],
        )
        .unwrap();
}

fn remove_session_issuance_account(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute(
            "DELETE FROM weavelit_account WHERE account_id = ?1",
            [identifier(1).as_bytes().as_slice()],
        )
        .unwrap();
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
                component: name("totp"),
                key: ConfigurationKey::new("mfa-module.enabled").unwrap(),
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
                mfa_required: true,
                credential_revision: CredentialRevision::from_value(u64::MAX).unwrap(),
                must_change_password: true,
                temporary_credential_expiration: Some(
                    TemporaryCredentialExpiration::from_unix_milliseconds(1_700_000_000_000)
                        .unwrap(),
                ),
            },
            Account {
                identifier: identifier(2),
                username: name("管理者-équipe"),
                display_name: Some(name("運用担当")),
                active: false,
                mfa_required: false,
                credential_revision: CredentialRevision::INITIAL,
                must_change_password: false,
                temporary_credential_expiration: None,
            },
        ],
        account_public_identities: vec![
            AccountPublicIdentity::new(identifier(1), account_public_identifier(0x91)),
            AccountPublicIdentity::new(identifier(2), account_public_identifier(0x92)),
        ],
        account_audit_references: vec![
            AccountAuditReference::new(identifier(1), audit_reference(0xA1)),
            AccountAuditReference::new(identifier(2), audit_reference(0xA2)),
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
                name: name("運用-équipe"),
                description: None,
            },
        ],
        group_audit_references: vec![
            GroupAuditReference::new(identifier(3), audit_reference(0xB3)),
            GroupAuditReference::new(identifier(7), audit_reference(0xB7)),
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
        log_configuration_audit_references: vec![LogConfigurationAuditReference::new(
            identifier(6),
            audit_reference(0xC6),
        )],
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
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &checkpoint,
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
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
/// sessions, queryable Log destination records, Log Module credentials,
/// generation history, and opaque terminal recovery rows are not part of
/// restorable state and cannot ride in a backup. It no longer conflates that
/// intent with absence of live operational tables.
#[test]
fn live_operational_tables_remain_outside_restorable_state_and_log_destination_storage() {
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
        let (_, column_name) = lower_column
            .split_once('.')
            .expect("the inventory must qualify every column with its table");
        assert!(!column_name.contains("session"));
        assert!(!column_name.contains("token"));
        assert!(!column_name.contains("csrf"));
    }
    let log_module_columns = column_names(&path)
        .into_iter()
        .filter(|column| {
            column.starts_with("weavelit_log_module_")
                || column.starts_with("weavelit_log_configuration_")
        })
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
        String::from("運用-équipe"),
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

    // Restored state explicitly leaves TOTP disabled until an Administrator
    // enables it.
    assert_eq!(
        database.load_component_enablement().unwrap(),
        ComponentEnablement::new([(ComponentKind::MfaModule, name("totp"))])
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

    // The seeded state carries the canonical TOTP disablement and the test
    // adds one Client Module disablement. No other setting is projected.
    assert_eq!(
        disabled,
        vec![
            (ComponentKind::ClientModule, String::from("web-ui")),
            (ComponentKind::MfaModule, String::from("totp")),
        ]
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

/// The two names the TOTP Module is addressed by.
fn totp_target() -> MfaModuleTarget {
    MfaModuleTarget {
        module: name("totp"),
        component: name("totp"),
    }
}

/// Enables the TOTP Module, which an acceptance is decided against.
fn enable_totp_module(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO weavelit_configuration \
             (component, setting_key, setting_value) \
             VALUES ('totp', 'mfa-module.enabled', ?1)",
            [COMPONENT_ENABLED_VALUE],
        )
        .unwrap();
}

#[test]
fn a_restore_clears_every_live_session_inside_the_state_replacement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(12);
    let mut database = pending_database(&path, deployment_identifier);
    insert_session_issuance_account(&path);
    database.create(&session(0x21, 0x22)).unwrap();
    database.create(&session(0x23, 0x24)).unwrap();
    assert_eq!(session_count(&path), 2);
    remove_session_issuance_account(&path);

    database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap();

    assert_eq!(session_count(&path), 0);
    assert_eq!(
        database
            .load_initialized_state(
                &account_public_identifier_persistence(),
                &audit_reference_persistence(),
                deployment_identifier,
            )
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
    insert_session_issuance_account(&path);
    // Accepting a step is one decision with the Module's enabled state and the
    // session it issues, so the Module is enabled here and each acceptance
    // carries its own session.
    enable_totp_module(&path);
    let step = MfaTimeStep::from_step(41_152_263).unwrap();
    database
        .accept_step(&totp_target(), identifier(5), step, &session(0x31, 0x32))
        .unwrap();
    database
        .accept_step(&totp_target(), identifier(7), step, &session(0x33, 0x34))
        .unwrap();
    assert_eq!(watermark_count(&path), 2);
    remove_session_issuance_account(&path);

    // The replacement writes the restored state's own configuration into a
    // table it expects to be empty, so the entry that enabled the Module for
    // this seeding is removed before it runs.
    Connection::open(&path)
        .unwrap()
        .execute(
            "DELETE FROM weavelit_configuration WHERE component = 'totp' \
             AND setting_key = 'mfa-module.enabled'",
            [],
        )
        .unwrap();
    database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap();

    assert_eq!(watermark_count(&path), 0);
    assert_eq!(database.accepted_step(identifier(5)).unwrap(), None);
    // The replacement wrote the restored state's own configuration, which
    // leaves the Module disabled, so enabling it again is what lets the same
    // step be offered back through the contract.
    enable_totp_module(&path);
    assert_eq!(
        database
            .accept_step(&totp_target(), identifier(5), step, &session(0x35, 0x36))
            .unwrap(),
        weavelit_server_database::MfaAcceptance::Accepted
    );
}

#[test]
fn a_rejected_state_replacement_leaves_live_sessions_untouched() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(13);
    let mut database = pending_database(&path, deployment_identifier);
    insert_session_issuance_account(&path);
    database.create(&session(0x25, 0x26)).unwrap();
    remove_session_issuance_account(&path);

    let error = database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &WorkflowCheckpoint::new(
                deployment_identifier,
                WorkflowKind::Restore,
                CheckpointMetadata::from_bytes(b"other-metadata".as_slice()).unwrap(),
            ),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
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
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            deployment_identifier,
        )
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
fn fresh_init_seeds_version_one_and_reads_history_across_restart() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(30);
    let checkpoint = WorkflowCheckpoint::new(
        deployment_identifier,
        WorkflowKind::Init,
        CheckpointMetadata::from_bytes(b"init-generation-checkpoint".as_slice()).unwrap(),
    );
    let mut database = SqliteDatabase::open(&path).unwrap();
    database.create_checkpoint(&checkpoint).unwrap();
    database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &checkpoint,
            &application_state(WorkflowKind::Init),
            &reconciliation_digest(0x30),
        )
        .unwrap();

    let persistence = log_configuration_generation_persistence();
    let initial_key = persistence.key(identifier(6), LogConfigurationVersion::INITIAL);
    let current = database
        .log_configuration_generations()
        .unwrap()
        .load_current_audit_log_configuration_generation(persistence)
        .unwrap()
        .unwrap();
    assert_eq!(current.key(), initial_key);
    assert_eq!(current.module().as_str(), "log-sqlite");
    assert_eq!(current.name().as_str(), "local");
    assert!(current.enabled());
    assert_eq!(current.settings().len(), 1);
    assert_eq!(current.settings()[0].key.as_str(), "retention");
    assert_eq!(current.settings()[0].value.as_str(), "unsupported");
    assert_eq!(current.log_types(), [LogType::System, LogType::Audit]);
    assert_eq!(
        database
            .log_configuration_generations()
            .unwrap()
            .load_log_configuration_generation(persistence, initial_key)
            .unwrap(),
        Some(current.clone())
    );
    drop(database);

    let historical_version = LogConfigurationVersion::new(u64::MAX).unwrap();
    let historical_bytes = historical_version.get().to_be_bytes();
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation \
             (configuration_id, generation_version, module, name, enabled) \
             VALUES (?1, ?2, 'log-sqlite', 'historical-local', 0)",
            rusqlite::params![
                identifier(6).as_bytes().as_slice(),
                historical_bytes.as_slice()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation_setting \
             (configuration_id, generation_version, setting_key, setting_value) \
             VALUES (?1, ?2, 'retention', 'historical')",
            rusqlite::params![
                identifier(6).as_bytes().as_slice(),
                historical_bytes.as_slice()
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_log_configuration_generation_log_type \
             (configuration_id, generation_version, log_type) VALUES (?1, ?2, 'audit')",
            rusqlite::params![
                identifier(6).as_bytes().as_slice(),
                historical_bytes.as_slice()
            ],
        )
        .unwrap();
    drop(connection);

    for _ in 0..2 {
        let mut reopened = SqliteDatabase::open(&path).unwrap();
        let historical_key = persistence.key(identifier(6), historical_version);
        let historical = reopened
            .log_configuration_generations()
            .unwrap()
            .load_log_configuration_generation(persistence, historical_key)
            .unwrap()
            .unwrap();
        assert_eq!(historical.key(), historical_key);
        assert_eq!(historical.name().as_str(), "historical-local");
        assert!(!historical.enabled());
        assert_eq!(historical.log_types(), [LogType::Audit]);
        assert_eq!(
            reopened
                .log_configuration_generations()
                .unwrap()
                .load_current_audit_log_configuration_generation(persistence)
                .unwrap()
                .unwrap()
                .key(),
            initial_key
        );
        assert_eq!(
            reopened
                .log_configuration_generations()
                .unwrap()
                .load_log_configuration_generation(
                    persistence,
                    persistence.key(identifier(6), LogConfigurationVersion::new(2).unwrap(),),
                )
                .unwrap(),
            None
        );
        drop(reopened);
    }
}

#[test]
fn generation_seed_failure_rolls_back_the_complete_checkpoint() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(31);
    let mut database = pending_database(&path, deployment_identifier);
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER test_reject_generation_seed \
             BEFORE INSERT ON weavelit_log_configuration_generation \
             BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
        )
        .unwrap();

    assert_eq!(
        database.complete_checkpoint(
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0x31),
        ),
        Err(DatabaseError::IntegrityFailure)
    );
    drop(database);

    assert_no_state_rows(&path);
    assert!(matches!(
        SqliteDatabase::open(&path),
        Err(DatabaseError::IntegrityFailure)
    ));
    Connection::open(&path)
        .unwrap()
        .execute_batch("DROP TRIGGER test_reject_generation_seed;")
        .unwrap();
    let reopened = SqliteDatabase::open(&path).unwrap();
    assert_eq!(
        reopened.inspect(deployment_identifier).unwrap(),
        DatabaseInspection::Pending(restore_checkpoint(deployment_identifier))
    );
}

#[test]
fn current_generation_reads_fail_closed_for_inconsistent_persistence() {
    let mutations = [
        (
            "malformed version",
            "PRAGMA foreign_keys = OFF; PRAGMA ignore_check_constraints = ON; \
             UPDATE weavelit_log_configuration_current_generation \
             SET generation_version = zeroblob(8)",
        ),
        (
            "missing current pointer",
            "DELETE FROM weavelit_log_configuration_current_generation",
        ),
        (
            "configuration identity",
            "PRAGMA foreign_keys = OFF; \
             DROP TRIGGER weavelit_log_configuration_generation_reject_update; \
             UPDATE weavelit_log_configuration_generation \
             SET configuration_id = x'07070707070707070707070707070707'",
        ),
        (
            "module mismatch",
            "UPDATE weavelit_log_module_configuration SET module = 'other-module'",
        ),
        (
            "enabled mismatch",
            "UPDATE weavelit_log_module_configuration SET enabled = 0",
        ),
        (
            "settings mismatch",
            "UPDATE weavelit_log_module_setting SET setting_value = 'changed'",
        ),
        (
            "missing Audit membership",
            "DROP TRIGGER weavelit_log_configuration_generation_log_type_reject_delete; \
             DELETE FROM weavelit_log_configuration_generation_log_type \
             WHERE log_type = 'audit'",
        ),
    ];

    for (case, mutation) in mutations {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = restored_database(&path, deployment(32));
        Connection::open(&path)
            .unwrap()
            .execute_batch(mutation)
            .unwrap();

        assert_eq!(
            database
                .log_configuration_generations()
                .unwrap()
                .load_current_audit_log_configuration_generation(
                    log_configuration_generation_persistence(),
                ),
            Err(DatabaseError::IntegrityFailure),
            "for {case}"
        );
    }
}

#[test]
fn exact_generation_read_rejects_malformed_settings() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(33));
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "DROP TRIGGER weavelit_log_configuration_generation_setting_reject_update; \
             PRAGMA ignore_check_constraints = ON; \
             UPDATE weavelit_log_configuration_generation_setting SET setting_key = ''",
        )
        .unwrap();
    let persistence = log_configuration_generation_persistence();

    assert_eq!(
        database
            .log_configuration_generations()
            .unwrap()
            .load_log_configuration_generation(
                persistence,
                persistence.key(identifier(6), LogConfigurationVersion::INITIAL),
            ),
        Err(DatabaseError::IntegrityFailure)
    );
}

#[test]
fn normalized_restore_state_excludes_generation_history() {
    let source_directory = tempfile::tempdir().unwrap();
    let source_path = database_path(&source_directory);
    let source_deployment = deployment(34);
    let mut source = restored_database(&source_path, source_deployment);
    let persistence = log_configuration_generation_persistence();
    let historical_version = LogConfigurationVersion::new(2).unwrap();
    let historical_bytes = historical_version.get().to_be_bytes();
    Connection::open(&source_path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_log_configuration_generation \
             (configuration_id, generation_version, module, name, enabled) \
             VALUES (?1, ?2, 'log-sqlite', 'source-history', 1)",
            rusqlite::params![
                identifier(6).as_bytes().as_slice(),
                historical_bytes.as_slice()
            ],
        )
        .unwrap();
    let loaded = source
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            source_deployment,
        )
        .unwrap();
    assert!(
        source
            .log_configuration_generations()
            .unwrap()
            .load_log_configuration_generation(
                persistence,
                persistence.key(identifier(6), historical_version),
            )
            .unwrap()
            .is_some()
    );

    let replacement_directory = tempfile::tempdir().unwrap();
    let replacement_path = database_path(&replacement_directory);
    let replacement_deployment = deployment(35);
    let mut replacement = SqliteDatabase::open(&replacement_path).unwrap();
    let checkpoint = restore_checkpoint(replacement_deployment);
    replacement.create_checkpoint(&checkpoint).unwrap();
    replacement
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &checkpoint,
            loaded.state(),
            &reconciliation_digest(0x35),
        )
        .unwrap();

    assert_eq!(
        replacement
            .load_initialized_state(
                &account_public_identifier_persistence(),
                &audit_reference_persistence(),
                replacement_deployment,
            )
            .unwrap()
            .state(),
        loaded.state()
    );
    assert_eq!(
        replacement
            .log_configuration_generations()
            .unwrap()
            .load_log_configuration_generation(
                persistence,
                persistence.key(identifier(6), historical_version),
            )
            .unwrap(),
        None
    );
    assert_eq!(
        Connection::open(&replacement_path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_log_configuration_generation",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );
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
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            deployment_identifier,
        )
        .unwrap();

    assert_eq!(loaded.deployment_identifier(), deployment_identifier);
    assert!(!loaded.completion_acknowledged());
    assert_eq!(loaded.state(), &expected);
    assert_eq!(loaded.state().accounts().len(), 2);
    assert_eq!(
        loaded.state().accounts()[0].credential_revision,
        CredentialRevision::from_value(u64::MAX).unwrap()
    );
    assert!(loaded.state().accounts()[0].must_change_password);
    assert_eq!(
        loaded.state().accounts()[0]
            .temporary_credential_expiration
            .unwrap()
            .as_unix_milliseconds(),
        1_700_000_000_000
    );
    assert_eq!(
        loaded.state().account_public_identities(),
        expected.account_public_identities()
    );
    assert_eq!(
        loaded.state().accounts()[1].username.as_str(),
        "管理者-équipe"
    );
    assert_eq!(loaded.state().groups()[1].name.as_str(), "運用-équipe");
    assert_eq!(loaded.state().account_audit_references().len(), 2);
    assert_eq!(loaded.state().group_audit_references().len(), 2);
    assert_eq!(
        loaded.state().log_configuration_audit_references(),
        [LogConfigurationAuditReference::new(
            identifier(6),
            audit_reference(0xC6)
        )]
    );
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
fn account_public_identity_is_persistent_unique_and_exactly_lookupable() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    drop(restored_database(&path, deployment(24)));
    let mut database = SqliteDatabase::open(&path).unwrap();

    let public_identifier = account_public_identifier(0x91);
    let identity = database
        .load_account_public_identity(&account_public_identifier_persistence(), public_identifier)
        .unwrap()
        .expect("the exact public identifier resolves to its account");

    assert_eq!(identity.account(), identifier(1));
    assert_eq!(identity.public_identifier(), public_identifier);
    assert_eq!(
        database.load_account_public_identity(
            &account_public_identifier_persistence(),
            account_public_identifier(0x99),
        ),
        Ok(None)
    );

    let connection = Connection::open(&path).unwrap();
    let index_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_schema \
             WHERE type = 'index' AND name = 'weavelit_account_public_identity_value'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert!(index_sql.contains("public_identifier"));
}

#[test]
fn account_administration_reads_are_ordered_exact_and_persistent() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    drop(restored_database(&path, deployment(27)));
    let mut database = SqliteDatabase::open(&path).unwrap();
    let persistence = account_public_identifier_persistence();

    let accounts = database
        .list_account_administration_projections(&persistence)
        .unwrap();
    assert_eq!(accounts.len(), 2);
    assert_eq!(accounts[0].username().as_str(), USERNAME);
    assert_eq!(
        accounts[0].display_name().map(Name::as_str),
        Some("First Admin")
    );
    assert!(accounts[0].active());
    assert!(accounts[0].mfa_required());
    assert_eq!(
        accounts[0].public_identifier(),
        account_public_identifier(0x91)
    );
    assert_eq!(accounts[1].username().as_str(), "管理者-équipe");
    assert_eq!(
        accounts[1].display_name().map(Name::as_str),
        Some("運用担当")
    );
    assert!(!accounts[1].active());
    assert!(!accounts[1].mfa_required());

    let exact = database
        .load_account_administration_projection(&persistence, account_public_identifier(0x92))
        .unwrap()
        .expect("the persisted public identifier must resolve after reopen");
    assert_eq!(exact, accounts[1]);
    assert_eq!(
        database
            .load_account_administration_projection(&persistence, account_public_identifier(0x99),),
        Ok(None)
    );

    let rendered = format!("{accounts:?}{exact:?}");
    for excluded in [
        VERIFIER,
        "ar-a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1",
        "protected-component-secret",
        "protected-factor-data",
        "protected-provider-credential",
        "session-marker-module",
    ] {
        assert!(
            !rendered.contains(excluded),
            "projection exposed {excluded}"
        );
    }
}

#[test]
fn account_administration_reads_fail_before_output_for_invalid_public_identity_state() {
    let corruptions = [
        "DROP TRIGGER weavelit_account_public_identity_reject_delete; \
         DELETE FROM weavelit_account_public_identity WHERE account_id = \
         x'01010101010101010101010101010101'",
        "PRAGMA ignore_check_constraints = ON; \
         DROP TRIGGER weavelit_account_public_identity_reject_update; \
         UPDATE weavelit_account_public_identity SET public_identifier = zeroblob(16) \
         WHERE account_id = x'01010101010101010101010101010101'",
        "DROP INDEX weavelit_account_public_identity_value; \
         DROP TRIGGER weavelit_account_public_identity_reject_update; \
         UPDATE weavelit_account_public_identity SET public_identifier = \
         x'92929292929292929292929292929292' \
         WHERE account_id = x'01010101010101010101010101010101'",
    ];

    for corruption in corruptions {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = restored_database(&path, deployment(28));
        Connection::open(&path)
            .unwrap()
            .execute_batch(corruption)
            .unwrap();
        let persistence = account_public_identifier_persistence();

        let list_error = database
            .list_account_administration_projections(&persistence)
            .unwrap_err();
        let lookup_error = database
            .load_account_administration_projection(&persistence, account_public_identifier(0x92))
            .unwrap_err();

        assert_eq!(list_error, DatabaseError::IntegrityFailure, "{corruption}");
        assert_eq!(
            lookup_error,
            DatabaseError::IntegrityFailure,
            "{corruption}"
        );
        assert_redacted(list_error);
        assert_redacted(lookup_error);
    }
}

#[test]
fn typed_audit_reference_projections_are_unique_persistent_and_indexed() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = restored_database(&path, deployment(25));

    let account = database
        .load_account_audit_reference(&audit_reference_persistence(), identifier(1))
        .unwrap()
        .expect("the account has an audit reference");
    let group = database
        .load_group_audit_reference(&audit_reference_persistence(), identifier(3))
        .unwrap()
        .expect("the Group has an audit reference");

    assert_eq!(account.account(), identifier(1));
    assert_eq!(account.audit_reference(), audit_reference(0xA1));
    assert_eq!(group.group(), identifier(3));
    assert_eq!(group.audit_reference(), audit_reference(0xB3));
    assert_ne!(account.audit_reference(), group.audit_reference());
    assert_eq!(
        database.load_account_audit_reference(&audit_reference_persistence(), identifier(9)),
        Ok(None)
    );
    assert_eq!(
        database.load_group_audit_reference(&audit_reference_persistence(), identifier(9)),
        Ok(None)
    );

    let connection = Connection::open(&path).unwrap();
    let account_index_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_schema \
             WHERE type = 'index' AND name = 'weavelit_account_audit_reference_value'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    let group_index_sql = connection
        .query_row(
            "SELECT sql FROM sqlite_schema \
             WHERE type = 'index' AND name = 'weavelit_group_audit_reference_value'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap();
    assert!(account_index_sql.contains("audit_reference"));
    assert!(group_index_sql.contains("audit_reference"));

    for (table, reference) in [
        (
            "weavelit_account_audit_reference",
            account.audit_reference(),
        ),
        ("weavelit_group_audit_reference", group.audit_reference()),
    ] {
        let plan = connection
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT * FROM {table} WHERE audit_reference = ?1"
            ))
            .unwrap()
            .query_map([reference.to_string()], |row| row.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join(" ");
        assert!(
            plan.contains("INDEX"),
            "lookup plan must use an index: {plan}"
        );
    }
}

#[test]
fn audit_reference_tables_enforce_typed_ownership_uniqueness_and_immutability() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    drop(SqliteDatabase::open(&path).unwrap());
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .unwrap();

    for (identifier, username) in [([1_u8; 16], "first"), ([2_u8; 16], "second")] {
        connection
            .execute(
                "INSERT INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, ?2, NULL, 1, 0)",
                rusqlite::params![identifier.as_slice(), username],
            )
            .unwrap();
    }
    for (identifier, name) in [([3_u8; 16], "first-group"), ([4_u8; 16], "second-group")] {
        connection
            .execute(
                "INSERT INTO weavelit_group (group_id, name, description) \
                 VALUES (?1, ?2, NULL)",
                rusqlite::params![identifier.as_slice(), name],
            )
            .unwrap();
    }

    let account_reference = "ar-11111111111111111111111111111111";
    let group_reference = "ar-22222222222222222222222222222222";
    connection
        .execute(
            "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
             VALUES (?1, ?2)",
            rusqlite::params![[1_u8; 16].as_slice(), account_reference],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) \
             VALUES (?1, ?2)",
            rusqlite::params![[3_u8; 16].as_slice(), group_reference],
        )
        .unwrap();

    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
                 VALUES (?1, 'ar-33333333333333333333333333333333')",
                [[9_u8; 16].as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) \
                 VALUES (?1, 'ar-44444444444444444444444444444444')",
                [[9_u8; 16].as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
                 VALUES (?1, ?2)",
                rusqlite::params![[2_u8; 16].as_slice(), account_reference],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) \
                 VALUES (?1, ?2)",
                rusqlite::params![[4_u8; 16].as_slice(), group_reference],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) \
                 VALUES (?1, ?2)",
                rusqlite::params![[4_u8; 16].as_slice(), account_reference],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
                 VALUES (?1, ?2)",
                rusqlite::params![[2_u8; 16].as_slice(), group_reference],
            )
            .is_err()
    );

    for malformed in [
        "ar-00000000000000000000000000000000",
        "ar-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "ar-1111111111111111111111111111111g",
        "ar-1111111111111111111111111111111",
        "11111111111111111111111111111111111",
    ] {
        assert!(
            connection
                .execute(
                    "INSERT INTO weavelit_account_audit_reference \
                     (account_id, audit_reference) VALUES (?1, ?2)",
                    rusqlite::params![[2_u8; 16].as_slice(), malformed],
                )
                .is_err(),
            "malformed value must fail its CHECK: {malformed}"
        );
    }

    assert!(
        connection
            .execute(
                "UPDATE weavelit_account_audit_reference \
                 SET audit_reference = 'ar-55555555555555555555555555555555' \
                 WHERE account_id = ?1",
                [[1_u8; 16].as_slice()],
            )
            .is_err()
    );
    assert!(
        connection
            .execute(
                "UPDATE weavelit_group_audit_reference SET group_id = ?1 WHERE group_id = ?2",
                rusqlite::params![[4_u8; 16].as_slice(), [3_u8; 16].as_slice()],
            )
            .is_err()
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
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap_err();

    assert_eq!(error, DatabaseError::AlreadyInitialized);
    assert_eq!(state_row_counts(&path), before);
    assert_redacted(error);
}

#[test]
fn completion_retains_only_its_reconciliation_digest() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(2);
    let mut database = restored_database(&path, deployment_identifier);

    assert!(
        database
            .matches_reconciliation(&reconciliation_digest(0xA0))
            .unwrap()
    );
    assert!(
        !database
            .matches_reconciliation(&reconciliation_digest(0xA1))
            .unwrap()
    );
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
            .complete_checkpoint(
                &account_public_identifier_persistence(),
                &checkpoint,
                &application_state(workflow),
                &reconciliation_digest(0xA0),
            )
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
            &account_public_identifier_persistence(),
            &restore_checkpoint(other),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap_err();
    let load_error = database
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            other,
        )
        .unwrap_err();
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
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap_err();
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    let obligation_error = database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Init),
            &reconciliation_digest(0xA0),
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
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
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
        reopened.load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            deployment_identifier,
        ),
        Err(DatabaseError::NotInitialized)
    );
}

#[test]
fn loading_uninitialized_or_pending_state_is_rejected() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(8);
    let mut database = SqliteDatabase::open(&path).unwrap();

    let uninitialized = database.load_initialized_state(
        &account_public_identifier_persistence(),
        &audit_reference_persistence(),
        deployment_identifier,
    );
    database
        .create_checkpoint(&restore_checkpoint(deployment_identifier))
        .unwrap();
    let pending = database.load_initialized_state(
        &account_public_identifier_persistence(),
        &audit_reference_persistence(),
        deployment_identifier,
    );

    assert_eq!(uninitialized, Err(DatabaseError::NotInitialized));
    assert_eq!(pending, Err(DatabaseError::NotInitialized));
}

#[test]
fn malformed_persisted_state_fails_integrity_validation() {
    let mutations = [
        "DROP TRIGGER weavelit_account_public_identity_reject_delete; \
         DELETE FROM weavelit_account_public_identity WHERE account_id = \
         x'01010101010101010101010101010101'",
        "INSERT INTO weavelit_account_public_identity (account_id, public_identifier) \
         VALUES (x'99999999999999999999999999999999', \
                 x'89898989898989898989898989898989')",
        "DELETE FROM weavelit_account_audit_reference",
        "INSERT INTO weavelit_account_audit_reference (account_id, audit_reference) \
         VALUES (x'99999999999999999999999999999999', \
         'ar-99999999999999999999999999999999')",
        "DELETE FROM weavelit_account_audit_reference WHERE account_id = \
         x'01010101010101010101010101010101'; \
         INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) \
         VALUES (x'01010101010101010101010101010101', \
         'ar-a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1')",
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
        connection
            .execute_batch("PRAGMA foreign_keys = OFF;")
            .unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);

        let error = match SqliteDatabase::open(&path) {
            Ok(mut database) => database
                .load_initialized_state(
                    &account_public_identifier_persistence(),
                    &audit_reference_persistence(),
                    deployment_identifier,
                )
                .unwrap_err(),
            Err(error) => error,
        };

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
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            deployment_identifier,
        )
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

/// Proves that live audit terminal obligations and supersessions (recovery
/// outbox rows) neither enter normalized restorable state nor are cleared by
/// checkpoint replacement. Obligations remain queryable after Restore and are
/// never imported from backup because they are live operational records, not
/// restorable application state.
#[test]
fn recovery_obligations_and_supersessions_are_live_operational_data_excluded_from_state() {
    type SupersessionRow = (
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
        Vec<u8>,
    );
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let deployment_identifier = deployment(26);
    let mut database = pending_database(&path, deployment_identifier);

    // Seed the live operational recovery tables before checkpoint completion.
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch("PRAGMA foreign_keys = ON;")
        .unwrap();

    let obligation_record_id: [u8; 16] = [0x0A; 16];
    let obligation_binding_id: [u8; 16] = [0x0B; 16];
    let obligation_binding_version: [u8; 8] = [0x0C; 8];
    let supersession_replacement_record_id: [u8; 16] = [0x0D; 16];
    let supersession_replacement_binding_id: [u8; 16] = [0x0E; 16];
    let supersession_replacement_binding_version: [u8; 8] = [0x0F; 8];

    // Insert a replacement obligation first to satisfy the foreign key constraint.
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                supersession_replacement_record_id.as_slice(),
                b"replacement-obligation-projection",
                supersession_replacement_binding_id.as_slice(),
                supersession_replacement_binding_version.as_slice(),
            ],
        )
        .unwrap();

    // Insert the original obligation: live operational recovery data that WILL NOT be
    // imported from backup and MUST NOT be cleared by state replacement.
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_obligation \
             (record_identifier, projection, binding_identifier, binding_version, acknowledged) \
             VALUES (?1, ?2, ?3, ?4, 0)",
            rusqlite::params![
                obligation_record_id.as_slice(),
                b"obligation-projection-data",
                obligation_binding_id.as_slice(),
                obligation_binding_version.as_slice(),
            ],
        )
        .unwrap();

    // Insert a supersession: immutable disposition record linking original to
    // replacement obligation. Also live operational data excluded from state.
    connection
        .execute(
            "INSERT INTO weavelit_audit_terminal_supersession \
             (original_record_identifier, disposition, \
              original_binding_identifier, original_binding_version, \
              replacement_record_identifier, \
              replacement_binding_identifier, replacement_binding_version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                obligation_record_id.as_slice(),
                b"supersession-disposition",
                obligation_binding_id.as_slice(),
                obligation_binding_version.as_slice(),
                supersession_replacement_record_id.as_slice(),
                supersession_replacement_binding_id.as_slice(),
                supersession_replacement_binding_version.as_slice(),
            ],
        )
        .unwrap();

    // Verify obligations and supersessions exist before state replacement.
    let obligation_count_before: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_audit_terminal_obligation",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let supersession_count_before: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_audit_terminal_supersession",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    assert_eq!(obligation_count_before, 2);
    assert_eq!(supersession_count_before, 1);

    // Complete the checkpoint: state replacement that atomically restores
    // application state from backup and seals the deployment. This MUST NOT
    // clear obligations/supersessions; they remain live operational data.
    database
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &restore_checkpoint(deployment_identifier),
            &application_state(WorkflowKind::Restore),
            &reconciliation_digest(0xA0),
        )
        .unwrap();

    // Verify obligations and supersessions are STILL present after restoration.
    let connection = Connection::open(&path).unwrap();
    let obligation_count_after: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_audit_terminal_obligation",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let supersession_count_after: i64 = connection
        .query_row(
            "SELECT count(*) FROM weavelit_audit_terminal_supersession",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(connection);

    assert_eq!(
        obligation_count_after, 2,
        "restoration must not clear recovery obligations"
    );
    assert_eq!(
        supersession_count_after, 1,
        "restoration must not clear recovery supersessions"
    );

    // Load current application state: excludes audit terminal obligations and
    // supersessions because they are live operational data. The existing guard
    // `normalized_state_and_backup_paths_do_not_reference_recovery_tables` proves
    // that backup excludes recovery tables; this test verifies live persistence.
    let loaded = database
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            deployment_identifier,
        )
        .unwrap();

    // Verify the normalized state matches expected application state.
    assert_eq!(
        loaded.state(),
        &application_state(WorkflowKind::Restore),
        "normalized restorable state must match restored aggregate"
    );

    // Directly verify obligation records persist with exact identifiers and binding.
    let connection = Connection::open(&path).unwrap();
    let obligations = {
        let mut stmt = connection
            .prepare(
                "SELECT record_identifier, binding_identifier, binding_version, acknowledged \
                 FROM weavelit_audit_terminal_obligation \
                 ORDER BY record_identifier",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get::<_, Vec<u8>>(0)?,
                row.get::<_, Vec<u8>>(1)?,
                row.get::<_, Vec<u8>>(2)?,
                row.get::<_, i32>(3)?,
            ))
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect::<Vec<_>>()
    };

    assert_eq!(obligations.len(), 2);
    // Verify original obligation unchanged.
    assert_eq!(obligations[0].0, obligation_record_id.to_vec());
    assert_eq!(obligations[0].1, obligation_binding_id.to_vec());
    assert_eq!(obligations[0].2, obligation_binding_version.to_vec());
    assert_eq!(obligations[0].3, 0, "acknowledged must remain 0");
    // Verify replacement obligation unchanged.
    assert_eq!(
        obligations[1].0,
        supersession_replacement_record_id.to_vec()
    );
    assert_eq!(
        obligations[1].1,
        supersession_replacement_binding_id.to_vec()
    );
    assert_eq!(
        obligations[1].2,
        supersession_replacement_binding_version.to_vec()
    );
    assert_eq!(obligations[1].3, 0, "acknowledged must remain 0");

    // Directly verify supersession disposition record unchanged.
    let supersession: SupersessionRow = connection
        .query_row(
            "SELECT original_record_identifier, disposition, \
             original_binding_identifier, original_binding_version, \
             replacement_record_identifier, \
             replacement_binding_identifier, replacement_binding_version \
             FROM weavelit_audit_terminal_supersession",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            },
        )
        .unwrap();

    assert_eq!(supersession.0, obligation_record_id.to_vec());
    assert_eq!(supersession.1, b"supersession-disposition");
    assert_eq!(supersession.2, obligation_binding_id.to_vec());
    assert_eq!(supersession.3, obligation_binding_version.to_vec());
    assert_eq!(supersession.4, supersession_replacement_record_id.to_vec());
    assert_eq!(supersession.5, supersession_replacement_binding_id.to_vec());
    assert_eq!(
        supersession.6,
        supersession_replacement_binding_version.to_vec()
    );
    drop(connection);
}

#[test]
fn created_and_reset_account_state_round_trips_through_normalized_restore() {
    let source_directory = tempfile::tempdir().unwrap();
    let source_path = database_path(&source_directory);
    let source_deployment = deployment(60);
    let mut source = restored_database(&source_path, source_deployment);
    let connection = Connection::open(&source_path).unwrap();
    connection
        .execute(
            "UPDATE weavelit_account SET active = 1 WHERE account_id = ?1",
            [identifier(2).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_password_verifier \
             (account_id, encoded_verifier) VALUES (?1, '$issuer-verifier')",
            [identifier(2).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, 'web-ui', 1000, 1000, 43201000)",
            rusqlite::params![
                [0x31_u8; SESSION_DIGEST_LENGTH].as_slice(),
                [0x32_u8; SESSION_DIGEST_LENGTH].as_slice(),
                identifier(2).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    drop(connection);

    let authority = ServerDatabaseAuthority::new();
    let terminal_persistence = AuditTerminalRecoveryPersistence::from_server_authority(&authority);
    let binding =
        StoredAuditDestinationBinding::from_persisted(&terminal_persistence, [0x71; 16], 1)
            .unwrap();
    let terminal = |identifier: u8| {
        ValidatedAuditTerminalObligationWrite::from_server_audit(
            &terminal_persistence,
            [identifier; 16],
            vec![identifier; 32],
            binding.clone(),
        )
        .unwrap()
    };
    let target = MfaModuleTarget {
        module: name("totp"),
        component: name("totp"),
    };
    let recheck = || {
        AccountCredentialIssuanceRecheck::new(
            identifier(2),
            SessionTokenHash::from_bytes([0x31; SESSION_DIGEST_LENGTH]).unwrap(),
            name("web-ui"),
            CredentialRevision::INITIAL,
            SessionInstant::from_unix_milliseconds(1_001).unwrap(),
            AccountCredentialIssuanceFactor::NoneObserved {
                target: target.clone(),
            },
        )
    };
    let created_account = identifier(8);
    let created_public_identifier = account_public_identifier(0x98);
    let created = AccountCreateMutation::new(
        recheck(),
        Account {
            identifier: created_account,
            username: name("restorable-created"),
            display_name: Some(name("Restorable Created")),
            active: true,
            mfa_required: false,
            credential_revision: CredentialRevision::INITIAL,
            must_change_password: true,
            temporary_credential_expiration: Some(
                TemporaryCredentialExpiration::from_unix_milliseconds(86_401_001).unwrap(),
            ),
        },
        AccountPublicIdentity::new(created_account, created_public_identifier),
        AccountAuditReference::new(created_account, audit_reference(0xD8)),
        AccountPasswordVerifier {
            account: created_account,
            verifier: PasswordVerifier::new("$created-verifier").unwrap(),
        },
    )
    .unwrap();
    let create_succeeded = terminal(0x81);
    let create_conflict = terminal(0x82);
    let create_denied = terminal(0x83);
    assert_eq!(
        source.create_account(
            &account_public_identifier_persistence(),
            &created,
            &AccountCredentialAuditTerminalWrites::new(
                &create_succeeded,
                &create_conflict,
                &create_denied,
            ),
        ),
        Ok(AccountCreateOutcome::Created)
    );

    let reset_target = source
        .prepare_password_reset_target(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            created_public_identifier,
        )
        .unwrap()
        .unwrap();
    let reset = AccountPasswordResetMutation::new(
        recheck(),
        reset_target,
        TemporaryCredentialExpiration::from_unix_milliseconds(86_402_001).unwrap(),
        AccountPasswordVerifier {
            account: created_account,
            verifier: PasswordVerifier::new("$reset-verifier").unwrap(),
        },
    )
    .unwrap();
    let reset_succeeded = terminal(0x84);
    let reset_conflict = terminal(0x85);
    let reset_denied = terminal(0x86);
    assert!(matches!(
        source.reset_account_password(
            &account_public_identifier_persistence(),
            &reset,
            &AccountCredentialAuditTerminalWrites::new(
                &reset_succeeded,
                &reset_conflict,
                &reset_denied,
            ),
        ),
        Ok(AccountPasswordResetOutcome::Reset { .. })
    ));

    let normalized = source
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            source_deployment,
        )
        .unwrap();
    let created_state = normalized
        .state()
        .accounts()
        .iter()
        .find(|account| account.identifier == created_account)
        .unwrap();
    assert_eq!(
        created_state.credential_revision,
        CredentialRevision::from_value(2).unwrap()
    );
    assert!(created_state.must_change_password);
    assert_eq!(
        created_state
            .temporary_credential_expiration
            .unwrap()
            .as_unix_milliseconds(),
        86_402_001
    );
    assert_eq!(
        normalized
            .state()
            .password_verifiers()
            .iter()
            .find(|verifier| verifier.account == created_account)
            .unwrap()
            .verifier
            .as_str(),
        "$reset-verifier"
    );

    let destination_directory = tempfile::tempdir().unwrap();
    let destination_path = database_path(&destination_directory);
    let destination_deployment = deployment(61);
    let mut destination = SqliteDatabase::open(&destination_path).unwrap();
    let checkpoint = restore_checkpoint(destination_deployment);
    destination.create_checkpoint(&checkpoint).unwrap();
    destination
        .complete_checkpoint(
            &account_public_identifier_persistence(),
            &checkpoint,
            normalized.state(),
            &reconciliation_digest(0xE1),
        )
        .unwrap();
    let restored = destination
        .load_initialized_state(
            &account_public_identifier_persistence(),
            &audit_reference_persistence(),
            destination_deployment,
        )
        .unwrap();
    assert_eq!(restored.state(), normalized.state());

    let source_connection = Connection::open(&source_path).unwrap();
    assert_eq!(
        source_connection
            .query_row("SELECT count(*) FROM weavelit_session", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        source_connection
            .query_row(
                "SELECT count(*) FROM weavelit_audit_terminal_obligation",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        2
    );
    let destination_connection = Connection::open(&destination_path).unwrap();
    for table in [
        "weavelit_session",
        "weavelit_mfa_replay_watermark",
        "weavelit_audit_terminal_obligation",
    ] {
        assert_eq!(
            destination_connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            0,
            "{table} must not transfer through normalized Restore state"
        );
    }
}
