use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension as _, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, ApplicationDatabase,
    AuditReferenceIdentifier, AuditReferencePersistence, AuditTerminalRecoveryPersistence,
    AuditTerminalRecoveryStore, AuditTerminalReplayBatchSize, DatabaseError, MfaModuleTarget,
    MfaPolicyAction, MfaPolicyAuditTerminalWrites, MfaPolicyMutation, MfaPolicyMutationOutcome,
    MfaPolicyRecheck, MfaPolicyWriterStore, Name, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
    SESSION_DIGEST_LENGTH, SessionInstant, SessionTokenHash, StateIdentifier,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACTOR: u8 = 1;
const TARGET: u8 = 2;
const ACTOR_FACTOR: u8 = 0x31;
const TARGET_FACTOR: u8 = 0x32;
const ACTOR_SESSION: u8 = 0x41;
const NOW: i64 = 1_001;

struct Surface {
    _directory: TempDir,
    path: PathBuf,
    database: SqliteDatabase,
    public_identifiers: AccountPublicIdentifierPersistence,
    audit_references: AuditReferencePersistence,
    recovery: AuditTerminalRecoveryPersistence,
}

struct Terminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

impl Terminals {
    fn writes(&self) -> MfaPolicyAuditTerminalWrites<'_> {
        MfaPolicyAuditTerminalWrites::new(&self.succeeded, &self.denied)
    }
}

fn surface(target_required: bool, target_enrolled: bool) -> Surface {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db");
    let database = SqliteDatabase::open(&path).unwrap();
    let authority = ServerDatabaseAuthority::new();
    let public_identifiers = AccountPublicIdentifierPersistence::from_server_authority(&authority);
    let audit_references = AuditReferencePersistence::from_server_authority(&authority);
    let recovery = AuditTerminalRecoveryPersistence::from_server_authority(&authority);

    insert_account(&path, &public_identifiers, &audit_references, ACTOR, false);
    insert_account(
        &path,
        &public_identifiers,
        &audit_references,
        TARGET,
        target_required,
    );
    enable_totp(&path);
    insert_factor(&path, ACTOR, ACTOR_FACTOR, 10);
    if target_enrolled {
        insert_factor(&path, TARGET, TARGET_FACTOR, 11);
    }
    insert_session(&path, ACTOR_SESSION, ACTOR);

    Surface {
        _directory: directory,
        path,
        database,
        public_identifiers,
        audit_references,
        recovery,
    }
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn public_identifier(
    persistence: &AccountPublicIdentifierPersistence,
    byte: u8,
) -> AccountPublicIdentifier {
    persistence.decode([byte; 16]).unwrap()
}

fn audit_reference(persistence: &AuditReferencePersistence, byte: u8) -> AuditReferenceIdentifier {
    persistence
        .decode(&format!("ar-{}", format!("{byte:02x}").repeat(16)))
        .unwrap()
}

fn insert_account(
    path: &Path,
    public_identifiers: &AccountPublicIdentifierPersistence,
    audit_references: &AuditReferencePersistence,
    byte: u8,
    required: bool,
) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required, credential_revision, \
              must_change_password, temporary_credential_expires_at_milliseconds) \
             VALUES (?1, ?2, NULL, 1, ?3, X'0000000000000001', 0, NULL)",
            params![
                identifier(byte).as_bytes().as_slice(),
                format!("user-{byte}"),
                i64::from(required)
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_public_identity \
             (account_id, public_identifier) VALUES (?1, ?2)",
            params![
                identifier(byte).as_bytes().as_slice(),
                public_identifiers
                    .encode(&public_identifier(public_identifiers, byte + 0x20))
                    .as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account_audit_reference \
             (account_id, audit_reference) VALUES (?1, ?2)",
            params![
                identifier(byte).as_bytes().as_slice(),
                audit_reference(audit_references, byte + 0x40).to_string(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_password_verifier \
             (account_id, encoded_verifier) VALUES (?1, ?2)",
            params![
                identifier(byte).as_bytes().as_slice(),
                format!("$verifier-{byte}")
            ],
        )
        .unwrap();
}

fn enable_totp(path: &Path) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_configuration (component, setting_key, setting_value) \
             VALUES ('totp', 'mfa-module.enabled', 'true')",
            [],
        )
        .unwrap();
}

fn insert_factor(path: &Path, account: u8, factor: u8, step: i64) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) \
             VALUES (?1, ?2, 'totp', X'010203')",
            params![
                identifier(factor).as_bytes().as_slice(),
                identifier(account).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_replay_watermark (factor_id, accepted_step) \
             VALUES (?1, ?2)",
            params![identifier(factor).as_bytes().as_slice(), step],
        )
        .unwrap();
}

fn insert_session(path: &Path, token: u8, account: u8) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, 'web-ui', 1000, 1000, ?4)",
            params![
                [token; SESSION_DIGEST_LENGTH].as_slice(),
                [token.wrapping_add(1); SESSION_DIGEST_LENGTH].as_slice(),
                identifier(account).as_bytes().as_slice(),
                1_000 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            ],
        )
        .unwrap();
}

fn mfa_target() -> MfaModuleTarget {
    MfaModuleTarget {
        module: Name::new("totp").unwrap(),
        component: Name::new("totp").unwrap(),
    }
}

fn recheck() -> MfaPolicyRecheck {
    MfaPolicyRecheck::new(
        identifier(ACTOR),
        SessionTokenHash::from_bytes([ACTOR_SESSION; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new("web-ui").unwrap(),
        mfa_target(),
        identifier(ACTOR_FACTOR),
        SessionInstant::from_unix_milliseconds(NOW).unwrap(),
    )
}

fn prepare(surface: &mut Surface) -> weavelit_server_database::MfaPolicyTarget {
    surface
        .database
        .prepare_mfa_policy_target(
            &surface.public_identifiers,
            &surface.audit_references,
            &Name::new("totp").unwrap(),
            public_identifier(&surface.public_identifiers, TARGET + 0x20),
        )
        .unwrap()
        .unwrap()
}

fn mutation(surface: &mut Surface, action: MfaPolicyAction) -> MfaPolicyMutation {
    MfaPolicyMutation::new(recheck(), prepare(surface), action).unwrap()
}

fn terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    byte: u8,
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x71; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [byte; 16],
        vec![byte; 32],
        binding,
    )
    .unwrap()
}

fn terminals(persistence: &AuditTerminalRecoveryPersistence, base: u8) -> Terminals {
    Terminals {
        succeeded: terminal(persistence, base),
        denied: terminal(persistence, base + 1),
    }
}

fn pending(surface: &mut Surface) -> Vec<[u8; 16]> {
    surface
        .database
        .list_pending_audit_terminal_obligations(
            &surface.recovery,
            AuditTerminalReplayBatchSize::new(16).unwrap(),
        )
        .unwrap()
        .into_iter()
        .map(|obligation| *obligation.identifier().as_bytes())
        .collect()
}

fn requirement(path: &Path) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT mfa_required FROM weavelit_account WHERE account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        != 0
}

fn session_count(path: &Path, account: u8) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM weavelit_session WHERE account_id = ?1",
            [identifier(account).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .unwrap()
}

fn factor(path: &Path, account: u8) -> Option<Vec<u8>> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT factor_id FROM weavelit_mfa_factor WHERE account_id = ?1 AND module = 'totp'",
            [identifier(account).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

fn watermark(path: &Path, factor: u8) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1",
            [identifier(factor).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

#[test]
fn requirement_changes_revoke_only_when_becoming_required() {
    let mut surface = surface(false, true);
    insert_session(&surface.path, 0x51, TARGET);
    insert_session(&surface.path, 0x52, TARGET);
    let require = mutation(
        &mut surface,
        MfaPolicyAction::Requirement { required: true },
    );
    let require_terminals = terminals(&surface.recovery, 0x81);

    assert_eq!(
        surface.database.change_mfa_policy(
            &surface.public_identifiers,
            &require,
            &require_terminals.writes(),
        ),
        Ok(MfaPolicyMutationOutcome::Changed {
            revoked_sessions: 2,
        })
    );
    assert!(requirement(&surface.path));
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(factor(&surface.path, TARGET), Some(vec![TARGET_FACTOR; 16]));
    assert_eq!(watermark(&surface.path, TARGET_FACTOR), Some(11));

    insert_session(&surface.path, 0x53, TARGET);
    let optional = mutation(
        &mut surface,
        MfaPolicyAction::Requirement { required: false },
    );
    let optional_terminals = terminals(&surface.recovery, 0x83);
    assert_eq!(
        surface.database.change_mfa_policy(
            &surface.public_identifiers,
            &optional,
            &optional_terminals.writes(),
        ),
        Ok(MfaPolicyMutationOutcome::Changed {
            revoked_sessions: 0,
        })
    );
    assert!(!requirement(&surface.path));
    assert_eq!(session_count(&surface.path, TARGET), 1);
    assert_eq!(pending(&mut surface), [[0x81; 16], [0x83; 16]]);
}

#[test]
fn reset_removes_factor_and_watermark_revokes_sessions_and_survives_restart() {
    let mut surface = surface(true, true);
    insert_session(&surface.path, 0x54, TARGET);
    let reset = mutation(&mut surface, MfaPolicyAction::EnrollmentReset);
    let reset_terminals = terminals(&surface.recovery, 0x85);

    assert_eq!(
        surface.database.change_mfa_policy(
            &surface.public_identifiers,
            &reset,
            &reset_terminals.writes(),
        ),
        Ok(MfaPolicyMutationOutcome::Changed {
            revoked_sessions: 1,
        })
    );
    assert!(requirement(&surface.path));
    assert_eq!(factor(&surface.path, TARGET), None);
    assert_eq!(watermark(&surface.path, TARGET_FACTOR), None);
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(pending(&mut surface), [[0x85; 16]]);

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    let prepared = prepare(&mut surface);
    assert!(prepared.required());
    assert_eq!(prepared.factor(), None);
}

#[test]
fn stale_target_and_final_issuer_state_commit_only_denied_terminal() {
    let mut stale = surface(true, true);
    let reset = mutation(&mut stale, MfaPolicyAction::EnrollmentReset);
    Connection::open(&stale.path)
        .unwrap()
        .execute(
            "DELETE FROM weavelit_mfa_factor WHERE account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
        )
        .unwrap();
    let stale_terminals = terminals(&stale.recovery, 0x87);
    assert_eq!(
        stale.database.change_mfa_policy(
            &stale.public_identifiers,
            &reset,
            &stale_terminals.writes(),
        ),
        Ok(MfaPolicyMutationOutcome::Stale)
    );
    assert!(requirement(&stale.path));
    assert_eq!(pending(&mut stale), [[0x88; 16]]);

    for (label, sql) in [
        (
            "missing factor",
            "DELETE FROM weavelit_mfa_factor WHERE account_id = X'01010101010101010101010101010101'",
        ),
        (
            "disabled module",
            "UPDATE weavelit_configuration SET setting_value = 'false' WHERE component = 'totp'",
        ),
        (
            "inactive actor",
            "UPDATE weavelit_account SET active = 0 WHERE account_id = X'01010101010101010101010101010101'",
        ),
        (
            "missing session",
            "DELETE FROM weavelit_session WHERE token_hash = X'4141414141414141414141414141414141414141414141414141414141414141'",
        ),
    ] {
        let mut denied = surface(false, true);
        let require = mutation(&mut denied, MfaPolicyAction::Requirement { required: true });
        Connection::open(&denied.path)
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        let denied_terminals = terminals(&denied.recovery, 0x89);
        assert_eq!(
            denied.database.change_mfa_policy(
                &denied.public_identifiers,
                &require,
                &denied_terminals.writes(),
            ),
            Ok(MfaPolicyMutationOutcome::Denied),
            "{label}"
        );
        assert!(!requirement(&denied.path), "{label}");
        assert_eq!(pending(&mut denied), [[0x8a; 16]], "{label}");
    }
}

#[test]
fn terminal_failure_rolls_back_requirement_and_session_revocation() {
    let mut surface = surface(false, true);
    insert_session(&surface.path, 0x55, TARGET);
    Connection::open(&surface.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_policy_terminal \
             BEFORE INSERT ON weavelit_audit_terminal_obligation \
             WHEN NEW.record_identifier = X'e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1' \
             BEGIN SELECT RAISE(ABORT, 'reject policy terminal'); END;",
        )
        .unwrap();
    let require = mutation(
        &mut surface,
        MfaPolicyAction::Requirement { required: true },
    );
    let terminals = terminals(&surface.recovery, 0xe1);

    assert_eq!(
        surface.database.change_mfa_policy(
            &surface.public_identifiers,
            &require,
            &terminals.writes(),
        ),
        Err(DatabaseError::IntegrityFailure)
    );
    assert!(!requirement(&surface.path));
    assert_eq!(session_count(&surface.path, TARGET), 1);
    assert!(pending(&mut surface).is_empty());
}

#[test]
fn application_database_trait_exposes_the_mfa_policy_writer() {
    let mut surface = surface(false, false);
    let database: &mut dyn ApplicationDatabase = &mut surface.database;
    assert!(database.mfa_policy_writers().is_some());
}
