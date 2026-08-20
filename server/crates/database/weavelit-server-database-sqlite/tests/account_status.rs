use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
};

use rusqlite::{Connection, OptionalExtension as _, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AccountPublicIdentifier, AccountPublicIdentifierPersistence, AccountStatus,
    AccountStatusAuditTerminalWrites, AccountStatusMutation, AccountStatusMutationOutcome,
    AccountStatusRecheck, AccountStatusWriterStore, ApplicationDatabase, AuditReferenceIdentifier,
    AuditReferencePersistence, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, CredentialRevision, DatabaseError, MfaAcceptance,
    MfaDirectSession, MfaEnrollment, MfaFactor, MfaModuleTarget, MfaStore, MfaTimeStep, Name,
    NewSession, ProtectedValue, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH,
    SessionCsrfHash, SessionInstant, SessionTokenHash, StateIdentifier,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACTOR: u8 = 1;
const TARGET: u8 = 2;
const ACTOR_SESSION: u8 = 0x31;
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

type PreservedTargetState = (
    String,
    Option<String>,
    i64,
    i64,
    Option<i64>,
    String,
    Vec<u8>,
    String,
    i64,
    Option<i64>,
    i64,
    i64,
);

impl Terminals {
    fn writes(&self) -> AccountStatusAuditTerminalWrites<'_> {
        AccountStatusAuditTerminalWrites::new(&self.succeeded, &self.denied)
    }
}

fn surface(target_active: bool) -> Surface {
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

    insert_account(
        &path,
        &public_identifiers,
        &audit_references,
        ACTOR,
        true,
        CredentialRevision::INITIAL,
    );
    insert_account(
        &path,
        &public_identifiers,
        &audit_references,
        TARGET,
        target_active,
        CredentialRevision::from_value(7).unwrap(),
    );
    insert_session(&path, ACTOR_SESSION, ACTOR, "web-ui", 1_000);

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
    active: bool,
    revision: CredentialRevision,
) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required, credential_revision, \
              must_change_password, temporary_credential_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, ?4, 0, ?5, 0, NULL)",
            params![
                identifier(byte).as_bytes().as_slice(),
                format!("user-{byte}"),
                format!("User {byte}"),
                i64::from(active),
                revision.to_stored_bytes().as_slice(),
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
                format!("$verifier-{byte}"),
            ],
        )
        .unwrap();
}

fn insert_session(path: &Path, token: u8, account: u8, client_module: &str, issued: i64) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_session \
             (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
              last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![
                [token; SESSION_DIGEST_LENGTH].as_slice(),
                [token.wrapping_add(1); SESSION_DIGEST_LENGTH].as_slice(),
                identifier(account).as_bytes().as_slice(),
                client_module,
                issued,
                issued + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            ],
        )
        .unwrap();
}

fn recheck(session: u8) -> AccountStatusRecheck {
    recheck_at(session, ACTOR, "web-ui", NOW)
}

fn recheck_at(session: u8, actor: u8, client_module: &str, now: i64) -> AccountStatusRecheck {
    AccountStatusRecheck::new(
        identifier(actor),
        SessionTokenHash::from_bytes([session; SESSION_DIGEST_LENGTH]).unwrap(),
        weavelit_server_database::Name::new(client_module).unwrap(),
        SessionInstant::from_unix_milliseconds(now).unwrap(),
    )
}

fn prepare(
    surface: &mut Surface,
    target: u8,
) -> Option<weavelit_server_database::AccountStatusTarget> {
    surface
        .database
        .prepare_account_status_target(
            &surface.public_identifiers,
            &surface.audit_references,
            public_identifier(&surface.public_identifiers, target + 0x20),
        )
        .unwrap()
}

fn mutation(
    surface: &mut Surface,
    target: u8,
    desired: AccountStatus,
    recheck: AccountStatusRecheck,
) -> AccountStatusMutation {
    AccountStatusMutation::new(recheck, prepare(surface, target).unwrap(), desired).unwrap()
}

fn change_status(surface: &mut Surface, desired: AccountStatus, terminal_base: u8) {
    let mutation = mutation(surface, TARGET, desired, recheck(ACTOR_SESSION));
    let terminals = terminals(&surface.recovery, terminal_base);
    assert!(matches!(
        surface.database.change_account_status(
            &surface.public_identifiers,
            &mutation,
            &terminals.writes(),
        ),
        Ok(AccountStatusMutationOutcome::Changed { .. })
    ));
}

fn mfa_target() -> MfaModuleTarget {
    MfaModuleTarget {
        module: Name::new("totp").unwrap(),
        component: Name::new("totp").unwrap(),
    }
}

fn issuance_session(token: u8, revision: u64) -> NewSession {
    NewSession::new(
        SessionTokenHash::from_bytes([token; SESSION_DIGEST_LENGTH]).unwrap(),
        SessionCsrfHash::from_bytes([token.wrapping_add(1); SESSION_DIGEST_LENGTH]).unwrap(),
        identifier(TARGET),
        CredentialRevision::from_value(revision).unwrap(),
        Name::new("web-ui").unwrap(),
        SessionInstant::from_unix_milliseconds(NOW).unwrap(),
    )
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

fn factor_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM weavelit_mfa_factor WHERE account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .unwrap()
}

fn accepted_step(path: &Path, factor: StateIdentifier) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1",
            [factor.as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

fn terminal(
    persistence: &AuditTerminalRecoveryPersistence,
    identifier: u8,
    projection: u8,
) -> ValidatedAuditTerminalObligationWrite {
    let binding =
        StoredAuditDestinationBinding::from_persisted(persistence, [0x71; 16], 1).unwrap();
    ValidatedAuditTerminalObligationWrite::from_server_audit(
        persistence,
        [identifier; 16],
        vec![projection; 32],
        binding,
    )
    .unwrap()
}

fn terminals(persistence: &AuditTerminalRecoveryPersistence, base: u8) -> Terminals {
    Terminals {
        succeeded: terminal(persistence, base, base),
        denied: terminal(persistence, base + 1, base + 1),
    }
}

fn pending_identifiers(surface: &mut Surface) -> Vec<[u8; 16]> {
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

fn status(path: &Path, account: u8) -> (bool, u64) {
    let (active, revision): (i64, Vec<u8>) = Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT active, credential_revision FROM weavelit_account WHERE account_id = ?1",
            [identifier(account).as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    (
        active != 0,
        u64::from_be_bytes(revision.try_into().unwrap()),
    )
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

fn migration_ledger(path: &Path) -> Vec<(i64, String, Vec<u8>)> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT sequence_number, identifier, checksum \
             FROM weavelit_migration_ledger ORDER BY sequence_number",
        )
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn preserved_target_state(path: &Path) -> PreservedTargetState {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT account.username, account.display_name, account.mfa_required, \
                    account.must_change_password, \
                    account.temporary_credential_expires_at_milliseconds, \
                    verifier.encoded_verifier, identity.public_identifier, \
                    reference.audit_reference, \
                    (SELECT count(*) FROM weavelit_mfa_factor WHERE account_id = account.account_id), \
                    (SELECT max(accepted_step) FROM weavelit_mfa_replay_watermark), \
                    (SELECT count(*) FROM weavelit_group_membership WHERE account_id = account.account_id), \
                    (SELECT count(*) FROM weavelit_group_grant) \
             FROM weavelit_account AS account \
             JOIN weavelit_password_verifier AS verifier ON verifier.account_id = account.account_id \
             JOIN weavelit_account_public_identity AS identity ON identity.account_id = account.account_id \
             JOIN weavelit_account_audit_reference AS reference ON reference.account_id = account.account_id \
             WHERE account.account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?,
                    row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?,
                ))
            },
        )
        .unwrap()
}

fn add_preserved_state(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "UPDATE weavelit_account SET mfa_required = 1, must_change_password = 1, \
             temporary_credential_expires_at_milliseconds = 9000 WHERE account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) VALUES (?1, ?2, 'totp', X'010203')",
            params![
                identifier(0x61).as_bytes().as_slice(),
                identifier(TARGET).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_replay_watermark (factor_id, accepted_step) VALUES (?1, 23)",
            [identifier(0x61).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group (group_id, name, description) \
             VALUES (?1, 'operators', 'Operators')",
            [identifier(0x71).as_bytes().as_slice()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_membership (group_id, account_id) VALUES (?1, ?2)",
            params![
                identifier(0x71).as_bytes().as_slice(),
                identifier(TARGET).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_group_grant (group_id, grant_kind, grant_value) \
             VALUES (?1, 'server_administration', '')",
            [identifier(0x71).as_bytes().as_slice()],
        )
        .unwrap();
}

#[test]
fn disable_and_reenable_preserve_state_revoke_sessions_and_survive_restart() {
    let mut surface = surface(true);
    add_preserved_state(&surface.path);
    insert_session(&surface.path, 0x41, TARGET, "web-ui", 1_000);
    insert_session(&surface.path, 0x42, TARGET, "other-client", 1_000);
    let preserved = preserved_target_state(&surface.path);
    let ledger = migration_ledger(&surface.path);
    let disable = mutation(
        &mut surface,
        TARGET,
        AccountStatus::Disabled,
        recheck(ACTOR_SESSION),
    );
    let disable_terminals = terminals(&surface.recovery, 0x81);

    assert_eq!(
        surface.database.change_account_status(
            &surface.public_identifiers,
            &disable,
            &disable_terminals.writes(),
        ),
        Ok(AccountStatusMutationOutcome::Changed {
            revoked_sessions: 2,
        })
    );
    assert_eq!(status(&surface.path, TARGET), (false, 8));
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(preserved_target_state(&surface.path), preserved);
    assert_eq!(migration_ledger(&surface.path), ledger);
    assert_eq!(pending_identifiers(&mut surface), [[0x81; 16]]);

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    let prepared = prepare(&mut surface, TARGET).unwrap();
    assert_eq!(prepared.status(), AccountStatus::Disabled);
    assert_eq!(
        prepared.credential_revision(),
        CredentialRevision::from_value(8).unwrap()
    );
    let reenable =
        AccountStatusMutation::new(recheck(ACTOR_SESSION), prepared, AccountStatus::Active)
            .unwrap();
    let reenable_terminals = terminals(&surface.recovery, 0x83);
    assert_eq!(
        surface.database.change_account_status(
            &surface.public_identifiers,
            &reenable,
            &reenable_terminals.writes(),
        ),
        Ok(AccountStatusMutationOutcome::Changed {
            revoked_sessions: 0,
        })
    );
    assert_eq!(status(&surface.path, TARGET), (true, 8));
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(preserved_target_state(&surface.path), preserved);
    assert_eq!(migration_ledger(&surface.path), ledger);
    assert_eq!(pending_identifiers(&mut surface), [[0x81; 16], [0x83; 16]]);
}

#[test]
fn self_disable_rechecks_then_revokes_the_issuer_session() {
    let mut surface = surface(true);
    let disable = mutation(
        &mut surface,
        ACTOR,
        AccountStatus::Disabled,
        recheck(ACTOR_SESSION),
    );
    let terminals = terminals(&surface.recovery, 0x85);

    assert_eq!(
        surface.database.change_account_status(
            &surface.public_identifiers,
            &disable,
            &terminals.writes(),
        ),
        Ok(AccountStatusMutationOutcome::Changed {
            revoked_sessions: 1,
        })
    );
    assert_eq!(status(&surface.path, ACTOR), (false, 2));
    assert_eq!(session_count(&surface.path, ACTOR), 0);
    assert_eq!(pending_identifiers(&mut surface), [[0x85; 16]]);
}

#[test]
fn missing_target_is_absent_before_audit_or_write() {
    let mut surface = surface(true);
    assert!(prepare(&mut surface, 0x70).is_none());
    assert!(pending_identifiers(&mut surface).is_empty());
}

#[test]
fn stale_target_and_issuer_denials_commit_only_the_denied_terminal() {
    let mut stale = surface(true);
    insert_session(&stale.path, 0x41, TARGET, "web-ui", 1_000);
    let disable = mutation(
        &mut stale,
        TARGET,
        AccountStatus::Disabled,
        recheck(ACTOR_SESSION),
    );
    Connection::open(&stale.path)
        .unwrap()
        .execute(
            "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
            params![
                identifier(TARGET).as_bytes().as_slice(),
                9_u64.to_be_bytes().as_slice(),
            ],
        )
        .unwrap();
    let stale_terminals = terminals(&stale.recovery, 0x87);
    assert_eq!(
        stale.database.change_account_status(
            &stale.public_identifiers,
            &disable,
            &stale_terminals.writes(),
        ),
        Ok(AccountStatusMutationOutcome::Stale)
    );
    assert_eq!(status(&stale.path, TARGET), (true, 9));
    assert_eq!(session_count(&stale.path, TARGET), 1);
    assert_eq!(pending_identifiers(&mut stale), [[0x88; 16]]);

    let cases = [
        ("missing session", 0x7f, ACTOR, "web-ui", NOW, false),
        ("wrong actor", ACTOR_SESSION, TARGET, "web-ui", NOW, false),
        ("wrong client", ACTOR_SESSION, ACTOR, "other", NOW, false),
        (
            "expired session",
            ACTOR_SESSION,
            ACTOR,
            "web-ui",
            1_000 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            false,
        ),
        ("inactive actor", ACTOR_SESSION, ACTOR, "web-ui", NOW, true),
    ];
    for (label, session, actor, client_module, now, deactivate_actor) in cases {
        let mut denied = surface(true);
        let disable = mutation(
            &mut denied,
            TARGET,
            AccountStatus::Disabled,
            recheck_at(session, actor, client_module, now),
        );
        if deactivate_actor {
            Connection::open(&denied.path)
                .unwrap()
                .execute(
                    "UPDATE weavelit_account SET active = 0 WHERE account_id = ?1",
                    [identifier(ACTOR).as_bytes().as_slice()],
                )
                .unwrap();
        }
        let denied_terminals = terminals(&denied.recovery, 0x89);
        assert_eq!(
            denied.database.change_account_status(
                &denied.public_identifiers,
                &disable,
                &denied_terminals.writes(),
            ),
            Ok(AccountStatusMutationOutcome::Denied),
            "{label}"
        );
        assert_eq!(status(&denied.path, TARGET), (true, 7), "{label}");
        assert_eq!(pending_identifiers(&mut denied), [[0x8a; 16]], "{label}");
    }
}

#[test]
fn terminal_failure_rolls_back_status_revision_and_session_revocation() {
    let mut surface = surface(true);
    insert_session(&surface.path, 0x41, TARGET, "web-ui", 1_000);
    Connection::open(&surface.path)
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_status_terminal \
             BEFORE INSERT ON weavelit_audit_terminal_obligation \
             WHEN NEW.record_identifier = X'e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1' \
             BEGIN SELECT RAISE(ABORT, 'reject status terminal'); END;",
        )
        .unwrap();
    let disable = mutation(
        &mut surface,
        TARGET,
        AccountStatus::Disabled,
        recheck(ACTOR_SESSION),
    );
    let terminals = terminals(&surface.recovery, 0xe1);

    assert_eq!(
        surface.database.change_account_status(
            &surface.public_identifiers,
            &disable,
            &terminals.writes(),
        ),
        Err(DatabaseError::IntegrityFailure)
    );
    assert_eq!(status(&surface.path, TARGET), (true, 7));
    assert_eq!(session_count(&surface.path, TARGET), 1);
    assert!(pending_identifiers(&mut surface).is_empty());
}

#[test]
fn competing_status_changes_commit_one_change_and_one_denial() {
    let mut surface = surface(true);
    insert_session(&surface.path, 0x41, TARGET, "web-ui", 1_000);
    let first_target = prepare(&mut surface, TARGET).unwrap();
    let second_target = prepare(&mut surface, TARGET).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for (target, base) in [(first_target, 0xa1), (second_target, 0xa3)] {
        let path = surface.path.clone();
        let barrier = Arc::clone(&barrier);
        let public_identifiers = surface.public_identifiers;
        let recovery = AuditTerminalRecoveryPersistence::from_server_authority(
            &ServerDatabaseAuthority::new(),
        );
        threads.push(std::thread::spawn(move || {
            let mutation =
                AccountStatusMutation::new(recheck(ACTOR_SESSION), target, AccountStatus::Disabled)
                    .unwrap();
            let terminals = terminals(&recovery, base);
            let mut database = SqliteDatabase::open(&path).unwrap();
            barrier.wait();
            database
                .change_account_status(&public_identifiers, &mutation, &terminals.writes())
                .unwrap()
        }));
    }
    let mut outcomes = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    outcomes.sort_by_key(|outcome| match outcome {
        AccountStatusMutationOutcome::Changed { .. } => 0,
        AccountStatusMutationOutcome::Stale => 1,
        AccountStatusMutationOutcome::Denied => 2,
    });

    assert_eq!(
        outcomes,
        [
            AccountStatusMutationOutcome::Changed {
                revoked_sessions: 1,
            },
            AccountStatusMutationOutcome::Stale,
        ]
    );
    assert_eq!(status(&surface.path, TARGET), (false, 8));
    assert_eq!(session_count(&surface.path, TARGET), 0);
    let pending = pending_identifiers(&mut surface);
    assert_eq!(pending.len(), 2);
    assert!(pending.contains(&[0xa1; 16]) || pending.contains(&[0xa3; 16]));
    assert!(pending.contains(&[0xa2; 16]) || pending.contains(&[0xa4; 16]));
}

#[test]
fn direct_issuance_rejects_a_pre_disable_revision_until_freshly_prepared() {
    let mut surface = surface(true);
    let stale_while_disabled = issuance_session(0x41, 7);
    change_status(&mut surface, AccountStatus::Disabled, 0xb1);
    assert_eq!(
        surface
            .database
            .issue_direct_session(&mfa_target(), &stale_while_disabled),
        Ok(MfaDirectSession::Denied)
    );

    change_status(&mut surface, AccountStatus::Active, 0xb3);
    assert_eq!(
        surface
            .database
            .issue_direct_session(&mfa_target(), &issuance_session(0x42, 7)),
        Ok(MfaDirectSession::Denied)
    );
    assert_eq!(
        surface
            .database
            .issue_direct_session(&mfa_target(), &issuance_session(0x43, 8)),
        Ok(MfaDirectSession::Issued)
    );
    assert_eq!(session_count(&surface.path, TARGET), 1);
}

#[test]
fn totp_issuance_rejects_a_pre_disable_revision_without_advancing_watermark() {
    let mut surface = surface(true);
    enable_totp(&surface.path);
    let factor = identifier(0x61);
    Connection::open(&surface.path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) \
             VALUES (?1, ?2, 'totp', X'010203')",
            params![
                factor.as_bytes().as_slice(),
                identifier(TARGET).as_bytes().as_slice(),
            ],
        )
        .unwrap();

    change_status(&mut surface, AccountStatus::Disabled, 0xb5);
    assert_eq!(
        surface.database.accept_step(
            &mfa_target(),
            factor,
            MfaTimeStep::from_step(10).unwrap(),
            &issuance_session(0x44, 7),
        ),
        Ok(MfaAcceptance::Rejected)
    );
    change_status(&mut surface, AccountStatus::Active, 0xb7);
    assert_eq!(
        surface.database.accept_step(
            &mfa_target(),
            factor,
            MfaTimeStep::from_step(10).unwrap(),
            &issuance_session(0x45, 7),
        ),
        Ok(MfaAcceptance::Rejected)
    );
    assert_eq!(accepted_step(&surface.path, factor), None);
    assert_eq!(
        surface.database.accept_step(
            &mfa_target(),
            factor,
            MfaTimeStep::from_step(10).unwrap(),
            &issuance_session(0x46, 8),
        ),
        Ok(MfaAcceptance::Accepted)
    );
    assert_eq!(accepted_step(&surface.path, factor), Some(10));
    assert_eq!(session_count(&surface.path, TARGET), 1);
}

#[test]
fn enrollment_issuance_rejects_a_pre_disable_revision_without_partial_state() {
    let mut surface = surface(true);
    enable_totp(&surface.path);
    let factor = MfaFactor {
        identifier: identifier(0x62),
        account: identifier(TARGET),
        module: Name::new("totp").unwrap(),
        protected_factor_data: ProtectedValue::new([0x55_u8; 20]).unwrap(),
    };

    change_status(&mut surface, AccountStatus::Disabled, 0xb9);
    assert_eq!(
        surface.database.enroll(
            &mfa_target(),
            &factor,
            MfaTimeStep::from_step(11).unwrap(),
            &issuance_session(0x47, 7),
        ),
        Ok(MfaEnrollment::Rejected)
    );
    change_status(&mut surface, AccountStatus::Active, 0xbb);
    assert_eq!(
        surface.database.enroll(
            &mfa_target(),
            &factor,
            MfaTimeStep::from_step(11).unwrap(),
            &issuance_session(0x48, 7),
        ),
        Ok(MfaEnrollment::Rejected)
    );
    assert_eq!(factor_count(&surface.path), 0);
    assert_eq!(accepted_step(&surface.path, factor.identifier), None);
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(
        surface.database.enroll(
            &mfa_target(),
            &factor,
            MfaTimeStep::from_step(11).unwrap(),
            &issuance_session(0x49, 8),
        ),
        Ok(MfaEnrollment::Enrolled)
    );
    assert_eq!(factor_count(&surface.path), 1);
    assert_eq!(accepted_step(&surface.path, factor.identifier), Some(11));
    assert_eq!(session_count(&surface.path, TARGET), 1);
}

#[test]
fn application_database_trait_exposes_the_status_writer_store() {
    let mut surface = surface(true);
    let database: &mut dyn ApplicationDatabase = &mut surface.database;
    assert!(database.account_status_writers().is_some());
}
