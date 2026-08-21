use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    ApplicationDatabase, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, COMPONENT_ENABLED_VALUE, CredentialRevision, DatabaseError,
    MfaAcceptance, MfaAdministrationStepUpAcceptance, MfaAdministrationStepUpRecheck,
    MfaDirectSession, MfaEnablementAuditTerminalWrites, MfaEnablementOutcome, MfaEnrollment,
    MfaFactor, MfaModuleTarget, MfaStore, MfaTimeStep, Name, NewSession, ProtectedValue,
    SESSION_DIGEST_LENGTH, SessionCsrfHash, SessionInstant, SessionStore, SessionTokenHash,
    StateIdentifier, StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const CSRF_BYTE: u8 = 0x7f;
const ISSUED_AT: i64 = 1_000;

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn factor(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn step(value: u64) -> MfaTimeStep {
    MfaTimeStep::from_step(value).unwrap()
}

fn target() -> MfaModuleTarget {
    MfaModuleTarget {
        module: Name::new("totp").unwrap(),
        component: Name::new("totp").unwrap(),
    }
}

/// The session one accepted code issues.
///
/// Each carries its own token digest, because an acceptance writes the session
/// in the transaction that records the step.
fn session(byte: u8) -> NewSession {
    session_at(byte, CredentialRevision::INITIAL, ISSUED_AT)
}

fn session_at(byte: u8, revision: CredentialRevision, issued_at: i64) -> NewSession {
    NewSession::new(
        SessionTokenHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap(),
        SessionCsrfHash::from_bytes([CSRF_BYTE; SESSION_DIGEST_LENGTH]).unwrap(),
        factor(byte),
        revision,
        Name::new("web-ui").unwrap(),
        SessionInstant::from_unix_milliseconds(issued_at).unwrap(),
    )
}

/// Opens a database whose MFA Module this deployment enables.
///
/// An acceptance is decided against the stored enabled state, so a test that
/// expects a code to be accepted enables the module first, exactly as a
/// deployment that verifies second factors has.
fn enabled_database(path: &Path) -> SqliteDatabase {
    let database = opened_with_accounts(path);
    set_enablement(path, COMPONENT_ENABLED_VALUE);
    database
}

fn opened_with_accounts(path: &Path) -> SqliteDatabase {
    let database = SqliteDatabase::open(path).unwrap();
    let connection = Connection::open(path).unwrap();
    for byte in [1_u8, 2] {
        connection
            .execute(
                "INSERT OR IGNORE INTO weavelit_account \
                 (account_id, username, display_name, active, mfa_required) \
                 VALUES (?1, ?2, NULL, 1, 0)",
                rusqlite::params![factor(byte).as_bytes().as_slice(), format!("user-{byte}")],
            )
            .unwrap();
    }
    database
}

fn set_enablement(path: &Path, value: &str) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT OR REPLACE INTO weavelit_configuration \
             (component, setting_key, setting_value) \
             VALUES ('totp', 'mfa-module.enabled', ?1)",
            [value],
        )
        .unwrap();
}

fn session_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM weavelit_session", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn factor_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM weavelit_mfa_factor", [], |row| {
            row.get(0)
        })
        .unwrap()
}

fn set_account_credential_state(
    path: &Path,
    account_byte: u8,
    active: bool,
    revision: u64,
    expiration: Option<i64>,
) {
    let connection = Connection::open(path).unwrap();
    if expiration.is_some() {
        connection
            .execute(
                "INSERT OR IGNORE INTO weavelit_password_verifier \
                 (account_id, encoded_verifier) VALUES (?1, '$test')",
                [factor(account_byte).as_bytes().as_slice()],
            )
            .unwrap();
    }
    connection
        .execute(
            "UPDATE weavelit_account SET active = ?2, credential_revision = ?3, \
             must_change_password = ?4, temporary_credential_expires_at_milliseconds = ?5 \
             WHERE account_id = ?1",
            rusqlite::params![
                factor(account_byte).as_bytes().as_slice(),
                i64::from(active),
                revision.to_be_bytes().as_slice(),
                i64::from(expiration.is_some()),
                expiration,
            ],
        )
        .unwrap();
}

type StoredCredentialState = (bool, u64, Option<i64>);
type IssuerCase = (&'static str, u8, Option<StoredCredentialState>, u64, bool);

fn issuer_cases() -> [IssuerCase; 5] {
    [
        ("absent", 3, None, 1, false),
        ("inactive", 1, Some((false, 1, None)), 1, false),
        ("stale revision", 1, Some((true, 2, None)), 1, false),
        (
            "at exact expiry",
            1,
            Some((true, 1, Some(ISSUED_AT))),
            1,
            false,
        ),
        (
            "before expiry",
            1,
            Some((true, 1, Some(ISSUED_AT + 1))),
            1,
            true,
        ),
    ]
}

#[test]
fn direct_session_issuance_rechecks_account_credential_state() {
    for (label, account_byte, stored, expected_revision, accepted) in issuer_cases() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = enabled_database(&path);
        if let Some((active, revision, expiration)) = stored {
            set_account_credential_state(&path, account_byte, active, revision, expiration);
        }

        let outcome = database
            .issue_direct_session(
                &target(),
                &session_at(
                    account_byte,
                    CredentialRevision::from_value(expected_revision).unwrap(),
                    ISSUED_AT,
                ),
            )
            .unwrap();

        assert_eq!(
            outcome,
            if accepted {
                MfaDirectSession::Issued
            } else {
                MfaDirectSession::Denied
            },
            "{label}"
        );
        assert_eq!(session_count(&path), i64::from(accepted), "{label}");
    }
}

#[test]
fn totp_issuance_rejects_account_changes_before_watermark_or_session_writes() {
    for (label, account_byte, stored, expected_revision, accepted) in issuer_cases() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = enabled_database(&path);
        if let Some((active, revision, expiration)) = stored {
            set_account_credential_state(&path, account_byte, active, revision, expiration);
        }

        let outcome = database
            .accept_step(
                &target(),
                factor(account_byte),
                step(41_152_263),
                &session_at(
                    account_byte,
                    CredentialRevision::from_value(expected_revision).unwrap(),
                    ISSUED_AT,
                ),
            )
            .unwrap();

        assert_eq!(
            outcome,
            if accepted {
                MfaAcceptance::Accepted
            } else {
                MfaAcceptance::Rejected
            },
            "{label}"
        );
        assert_eq!(session_count(&path), i64::from(accepted), "{label}");
        assert_eq!(
            stored_step(&path, factor(account_byte)),
            accepted.then_some(41_152_263),
            "{label}"
        );
    }
}

#[test]
fn enrollment_issuance_rejects_account_changes_before_factor_watermark_or_session_writes() {
    for (label, account_byte, stored, expected_revision, accepted) in issuer_cases() {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = enabled_database(&path);
        if let Some((active, revision, expiration)) = stored {
            set_account_credential_state(&path, account_byte, active, revision, expiration);
        }
        let enrolled_factor = MfaFactor {
            identifier: factor(account_byte.wrapping_add(0x40)),
            account: factor(account_byte),
            module: Name::new("totp").unwrap(),
            protected_factor_data: ProtectedValue::new([0x55_u8; 20]).unwrap(),
        };

        let outcome = database
            .enroll(
                &target(),
                &enrolled_factor,
                step(41_152_263),
                &session_at(
                    account_byte,
                    CredentialRevision::from_value(expected_revision).unwrap(),
                    ISSUED_AT,
                ),
            )
            .unwrap();

        assert_eq!(
            outcome,
            if accepted {
                MfaEnrollment::Enrolled
            } else {
                MfaEnrollment::Rejected
            },
            "{label}"
        );
        assert_eq!(factor_count(&path), i64::from(accepted), "{label}");
        assert_eq!(session_count(&path), i64::from(accepted), "{label}");
        assert_eq!(
            stored_step(&path, enrolled_factor.identifier),
            accepted.then_some(41_152_263),
            "{label}"
        );
    }
}

fn stored_step(path: &Path, factor: StateIdentifier) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1",
            [factor.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .ok()
}

fn administration_recheck(actor: u8, session_byte: u8, now: i64) -> MfaAdministrationStepUpRecheck {
    MfaAdministrationStepUpRecheck::new(
        factor(actor),
        SessionTokenHash::from_bytes([session_byte; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new("web-ui").unwrap(),
        SessionInstant::from_unix_milliseconds(now).unwrap(),
    )
}

fn stored_session_activity(path: &Path, token_byte: u8) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT last_seen_at_milliseconds FROM weavelit_session WHERE token_hash = ?1",
            [
                SessionTokenHash::from_bytes([token_byte; SESSION_DIGEST_LENGTH])
                    .unwrap()
                    .as_bytes()
                    .as_slice(),
            ],
            |row| row.get(0),
        )
        .ok()
}

#[test]
fn administration_step_up_advances_only_the_watermark_and_rejects_replay() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    insert_enrolled_account(&path, 1);
    SessionStore::create(&mut database, &session(1)).unwrap();

    assert_eq!(
        database
            .accept_administration_step_up(
                &target(),
                factor(0x41),
                step(41_152_263),
                &administration_recheck(1, 1, ISSUED_AT + 1),
            )
            .unwrap(),
        MfaAdministrationStepUpAcceptance::Accepted
    );
    assert_eq!(stored_step(&path, factor(0x41)), Some(41_152_263));
    assert_eq!(
        session_count(&path),
        1,
        "step-up must not rotate the session"
    );
    assert_eq!(stored_session_activity(&path, 1), Some(ISSUED_AT));
    assert_eq!(
        database
            .accept_administration_step_up(
                &target(),
                factor(0x41),
                step(41_152_263),
                &administration_recheck(1, 1, ISSUED_AT + 2),
            )
            .unwrap(),
        MfaAdministrationStepUpAcceptance::Replayed
    );
}

#[test]
fn administration_step_up_denies_disabled_stale_wrong_session_and_inactive_actor() {
    for (label, prepare, recheck, expected) in [
        (
            "wrong session",
            None,
            administration_recheck(1, 9, ISSUED_AT + 1),
            MfaAdministrationStepUpAcceptance::Rejected,
        ),
        (
            "stale session",
            None,
            administration_recheck(
                1,
                1,
                ISSUED_AT + weavelit_server_database::SESSION_IDLE_TIMEOUT_MILLISECONDS,
            ),
            MfaAdministrationStepUpAcceptance::Rejected,
        ),
        (
            "inactive actor",
            Some((false, COMPONENT_ENABLED_VALUE)),
            administration_recheck(1, 1, ISSUED_AT + 1),
            MfaAdministrationStepUpAcceptance::Rejected,
        ),
        (
            "disabled module",
            Some((true, "false")),
            administration_recheck(1, 1, ISSUED_AT + 1),
            MfaAdministrationStepUpAcceptance::ModuleDisabled,
        ),
    ] {
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = database_path(&temporary_directory);
        let mut database = enabled_database(&path);
        insert_enrolled_account(&path, 1);
        SessionStore::create(&mut database, &session(1)).unwrap();
        if let Some((active, enabled)) = prepare {
            set_account_credential_state(&path, 1, active, 1, None);
            set_enablement(&path, enabled);
        }

        assert_eq!(
            database
                .accept_administration_step_up(&target(), factor(0x41), step(41_152_264), &recheck,)
                .unwrap(),
            expected,
            "{label}"
        );
        assert_eq!(stored_step(&path, factor(0x41)), None, "{label}");
        assert_eq!(session_count(&path), 1, "{label}");
    }
}

fn enablement(path: &Path) -> Option<String> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT setting_value FROM weavelit_configuration \
             WHERE component = 'totp' AND setting_key = 'mfa-module.enabled'",
            [],
            |row| row.get(0),
        )
        .ok()
}

fn insert_enrolled_account(path: &Path, byte: u8) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required) \
             VALUES (?1, ?2, NULL, 1, 0)",
            rusqlite::params![factor(byte).as_bytes().as_slice(), format!("user-{byte}")],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) VALUES (?1, ?2, 'totp', ?3)",
            rusqlite::params![
                factor(byte.wrapping_add(0x40)).as_bytes().as_slice(),
                factor(byte).as_bytes().as_slice(),
                [0x55_u8; 20].as_slice()
            ],
        )
        .unwrap();
}

fn audit_persistence() -> AuditTerminalRecoveryPersistence {
    AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new())
}

fn audit_terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    identifier_byte: u8,
    projection: &[u8],
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x44; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [identifier_byte; 16],
        projection.to_vec(),
        binding,
    )
    .unwrap()
}

#[test]
fn enablement_persists_the_terminal_selected_by_the_transaction_outcome() {
    for (expected_enrolled, expected_outcome, expected_identifier) in [
        (
            0,
            MfaEnablementOutcome::Applied {
                revoked_sessions: 0,
            },
            1,
        ),
        (
            1,
            MfaEnablementOutcome::EnrolledCountChanged {
                current_affected_users: 0,
            },
            2,
        ),
    ] {
        let temporary_directory = tempfile::tempdir().unwrap();
        let mut database = SqliteDatabase::open(&database_path(&temporary_directory)).unwrap();
        let persistence = audit_persistence();
        let applied = audit_terminal(&persistence, 1, b"applied-terminal");
        let conflict = audit_terminal(&persistence, 2, b"conflict-terminal");
        let audit_terminals = MfaEnablementAuditTerminalWrites::new(&applied, &conflict);

        assert_eq!(
            database
                .set_module_enabled(&target(), true, expected_enrolled, &audit_terminals)
                .unwrap(),
            expected_outcome
        );
        let pending = database
            .list_pending_audit_terminal_obligations(
                &persistence,
                AuditTerminalReplayBatchSize::new(2).unwrap(),
            )
            .unwrap();

        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0].identifier().as_bytes(),
            &[expected_identifier; 16]
        );
        assert_eq!(
            enablement(&database_path(&temporary_directory)),
            (expected_enrolled == 0).then(|| "true".to_owned())
        );
    }
}

#[test]
fn disabling_recounts_users_revokes_their_sessions_and_commits_the_success_terminal() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    insert_enrolled_account(&path, 1);
    weavelit_server_database::SessionStore::create(&mut database, &session(1)).unwrap();
    let persistence = audit_persistence();
    let applied = audit_terminal(&persistence, 3, b"disable-applied");
    let conflict = audit_terminal(&persistence, 4, b"disable-conflict");
    let audit_terminals = MfaEnablementAuditTerminalWrites::new(&applied, &conflict);

    assert_eq!(database.enrolled_accounts(&target()).unwrap(), 1);
    assert_eq!(
        database
            .set_module_enabled(&target(), false, 1, &audit_terminals)
            .unwrap(),
        MfaEnablementOutcome::Applied {
            revoked_sessions: 1
        }
    );
    assert_eq!(enablement(&path).as_deref(), Some("false"));
    assert_eq!(session_count(&path), 0);
    let pending = database
        .list_pending_audit_terminal_obligations(
            &persistence,
            AuditTerminalReplayBatchSize::new(2).unwrap(),
        )
        .unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].identifier().as_bytes(), &[3; 16]);
}

#[test]
fn terminal_persistence_failure_rolls_back_enablement_sessions_and_obligation() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    insert_enrolled_account(&path, 1);
    weavelit_server_database::SessionStore::create(&mut database, &session(1)).unwrap();
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_mfa_audit_terminal \
             BEFORE INSERT ON weavelit_audit_terminal_obligation \
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        )
        .unwrap();
    let persistence = audit_persistence();
    let applied = audit_terminal(&persistence, 5, b"rollback-applied");
    let conflict = audit_terminal(&persistence, 6, b"rollback-conflict");
    let audit_terminals = MfaEnablementAuditTerminalWrites::new(&applied, &conflict);

    assert_eq!(
        database.set_module_enabled(&target(), false, 1, &audit_terminals),
        Err(DatabaseError::IntegrityFailure)
    );
    assert_eq!(enablement(&path).as_deref(), Some(COMPONENT_ENABLED_VALUE));
    assert_eq!(session_count(&path), 1);
    assert!(
        database
            .list_pending_audit_terminal_obligations(
                &persistence,
                AuditTerminalReplayBatchSize::new(2).unwrap(),
            )
            .unwrap()
            .is_empty()
    );
}

#[test]
fn a_factor_that_has_accepted_nothing_has_no_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let mut database = SqliteDatabase::open(&database_path(&temporary_directory)).unwrap();

    assert_eq!(database.accepted_step(factor(1)).unwrap(), None);
}

#[test]
fn the_first_step_is_accepted_and_becomes_the_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);

    let acceptance = database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();

    assert_eq!(acceptance, MfaAcceptance::Accepted);
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_263))
    );
    assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
    assert_eq!(
        session_count(&path),
        1,
        "the accepted step issues its session in the same transaction"
    );
}

/// A code presented after the Module was disabled is refused and writes nothing.
///
/// The step is fresh and would advance the watermark, so the only thing
/// refusing it is the enabled state the write itself is decided against.
/// Neither a watermark nor a session survives, which is what keeps a second
/// factor completed against a Module the deployment stopped verifying from
/// signing anyone in. The last row re-enables the Module and is accepted, so
/// the refusals are the disabled Module rather than the step or the factor.
#[test]
fn a_step_presented_after_the_module_was_disabled_is_refused_and_issues_no_session() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);

    for value in ["false", "yes", COMPONENT_ENABLED_VALUE] {
        set_enablement(&path, value);

        let acceptance = database
            .accept_step(&target(), factor(1), step(41_152_263), &session(1))
            .unwrap();

        if value == COMPONENT_ENABLED_VALUE {
            assert_eq!(acceptance, MfaAcceptance::Accepted);
            assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
            assert_eq!(session_count(&path), 1);
            continue;
        }
        assert_eq!(acceptance, MfaAcceptance::ModuleDisabled, "{value}");
        assert_eq!(stored_step(&path, factor(1)), None, "{value}");
        assert_eq!(session_count(&path), 0, "{value}");
    }
}

/// A Module with no stored enablement entry at all verifies nothing.
#[test]
fn a_step_is_refused_when_no_enablement_entry_exists() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened_with_accounts(&path);

    let acceptance = database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();

    assert_eq!(acceptance, MfaAcceptance::ModuleDisabled);
    assert_eq!(stored_step(&path, factor(1)), None);
    assert_eq!(session_count(&path), 0);
}

#[test]
fn a_step_that_does_not_advance_the_watermark_is_refused_as_a_replay() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();

    for presented in [41_152_263, 41_152_262, 0] {
        assert_eq!(
            database
                .accept_step(&target(), factor(1), step(presented), &session(2))
                .unwrap(),
            MfaAcceptance::Replayed
        );
        assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
        assert_eq!(
            session_count(&path),
            1,
            "a replayed code issues no further session"
        );
    }
}

#[test]
fn only_a_strictly_later_step_advances_the_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();

    let acceptance = database
        .accept_step(&target(), factor(1), step(41_152_264), &session(2))
        .unwrap();

    assert_eq!(acceptance, MfaAcceptance::Accepted);
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_264))
    );
}

#[test]
fn each_factor_keeps_its_own_watermark() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();

    assert_eq!(
        database
            .accept_step(&target(), factor(2), step(41_152_263), &session(2))
            .unwrap(),
        MfaAcceptance::Accepted
    );
    assert_eq!(
        database.accepted_step(factor(1)).unwrap(),
        Some(step(41_152_263))
    );
    assert_eq!(
        database.accepted_step(factor(2)).unwrap(),
        Some(step(41_152_263))
    );
}

#[test]
fn a_watermark_survives_reopening_the_database() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    database
        .accept_step(&target(), factor(1), step(66_666_666), &session(1))
        .unwrap();
    drop(database);

    let mut reopened = SqliteDatabase::open(&path).unwrap();

    assert_eq!(
        reopened.accepted_step(factor(1)).unwrap(),
        Some(step(66_666_666))
    );
    assert_eq!(
        reopened
            .accept_step(&target(), factor(1), step(66_666_666), &session(2))
            .unwrap(),
        MfaAcceptance::Replayed
    );
}

/// The schema, not only the calling code, refuses a reused or rewound step, so
/// no statement reaching this table can make a spent code usable again.
#[test]
fn a_direct_statement_cannot_reuse_or_rewind_an_accepted_step() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = enabled_database(&path);
    database
        .accept_step(&target(), factor(1), step(41_152_263), &session(1))
        .unwrap();
    drop(database);

    let connection = Connection::open(&path).unwrap();
    for presented in [41_152_263_i64, 41_152_262, 0] {
        let rejected = connection.execute(
            "UPDATE weavelit_mfa_replay_watermark SET accepted_step = ?2 WHERE factor_id = ?1",
            rusqlite::params![factor(1).as_bytes().as_slice(), presented],
        );

        assert!(rejected.is_err(), "the schema must refuse a reused step");
    }
    let negative = connection.execute(
        "INSERT INTO weavelit_mfa_replay_watermark (factor_id, accepted_step) VALUES (?1, -1)",
        [factor(2).as_bytes().as_slice()],
    );

    assert!(negative.is_err(), "the schema must refuse a negative step");
    assert_eq!(stored_step(&path, factor(1)), Some(41_152_263));
}

#[test]
fn the_backend_serves_its_replay_watermarks_through_the_contract() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut concrete = enabled_database(&path);
    let database: &mut dyn ApplicationDatabase = &mut concrete;

    let store = database
        .mfa()
        .expect("the SQLite backend serves watermarks");

    assert_eq!(
        store
            .accept_step(&target(), factor(1), step(1), &session(1))
            .unwrap(),
        MfaAcceptance::Accepted
    );
    assert_eq!(
        store
            .accept_step(&target(), factor(1), step(1), &session(2))
            .unwrap(),
        MfaAcceptance::Replayed
    );
}
