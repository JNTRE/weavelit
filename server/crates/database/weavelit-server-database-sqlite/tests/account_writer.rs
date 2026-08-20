use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

use rusqlite::{Connection, OptionalExtension as _, params};
use tempfile::TempDir;
use weavelit_server_database::{
    Account, AccountAuditReference, AccountCreateMutation, AccountCreateOutcome,
    AccountCredentialAuditTerminalWrites, AccountCredentialIssuanceFactor,
    AccountCredentialIssuanceRecheck, AccountCredentialWriterStore, AccountPasswordResetMutation,
    AccountPasswordResetOutcome, AccountPasswordVerifier, AccountPublicIdentifier,
    AccountPublicIdentifierPersistence, AccountPublicIdentity, ApplicationDatabase,
    AuditReferenceIdentifier, AuditReferencePersistence, AuditTerminalRecoveryPersistence,
    AuditTerminalRecoveryStore, AuditTerminalReplayBatchSize, COMPONENT_ENABLED_VALUE,
    CredentialRevision, DatabaseError, MfaModuleTarget, MfaTimeStep, Name, PasswordVerifier,
    SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH, SessionInstant,
    SessionTokenHash, StateIdentifier, StoredAuditDestinationBinding,
    TemporaryCredentialExpiration, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACTOR: u8 = 1;
const TARGET: u8 = 2;
const ACTOR_SESSION: u8 = 0x31;
const NOW: i64 = 1_001;
const EXPIRATION: i64 = NOW + 86_400_000;

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
    conflict: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

type StoredAccountCredentialState = (
    String,
    Option<String>,
    i64,
    i64,
    Vec<u8>,
    i64,
    Option<i64>,
    String,
);

impl Terminals {
    fn writes(&self) -> AccountCredentialAuditTerminalWrites<'_> {
        AccountCredentialAuditTerminalWrites::new(&self.succeeded, &self.conflict, &self.denied)
    }
}

fn surface(with_target: bool) -> Surface {
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
        "actor",
        "Actor",
        "$actor-verifier",
    );
    insert_session(&path, ACTOR_SESSION, ACTOR, "web-ui", 1_000);
    if with_target {
        insert_account(
            &path,
            &public_identifiers,
            &audit_references,
            TARGET,
            "target",
            "Target",
            "$target-verifier",
        );
    }

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

fn target() -> MfaModuleTarget {
    MfaModuleTarget {
        module: Name::new("totp").unwrap(),
        component: Name::new("totp").unwrap(),
    }
}

fn insert_account(
    path: &PathBuf,
    public_identifiers: &AccountPublicIdentifierPersistence,
    audit_references: &AuditReferencePersistence,
    byte: u8,
    username: &str,
    display_name: &str,
    verifier: &str,
) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_account \
             (account_id, username, display_name, active, mfa_required, credential_revision, \
              must_change_password, temporary_credential_expires_at_milliseconds) \
             VALUES (?1, ?2, ?3, 1, 0, ?4, 0, NULL)",
            params![
                identifier(byte).as_bytes().as_slice(),
                username,
                display_name,
                CredentialRevision::INITIAL.to_stored_bytes().as_slice(),
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
            params![identifier(byte).as_bytes().as_slice(), verifier],
        )
        .unwrap();
}

fn insert_session(path: &PathBuf, token: u8, account: u8, client_module: &str, issued: i64) {
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

fn recheck(session: u8) -> AccountCredentialIssuanceRecheck {
    recheck_at(session, ACTOR, "web-ui", NOW, CredentialRevision::INITIAL)
}

fn recheck_at(
    session: u8,
    actor: u8,
    client_module: &str,
    now: i64,
    revision: CredentialRevision,
) -> AccountCredentialIssuanceRecheck {
    AccountCredentialIssuanceRecheck::new(
        identifier(actor),
        SessionTokenHash::from_bytes([session; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new(client_module).unwrap(),
        revision,
        SessionInstant::from_unix_milliseconds(now).unwrap(),
        AccountCredentialIssuanceFactor::NoneObserved { target: target() },
    )
}

fn totp_recheck(step: u64) -> AccountCredentialIssuanceRecheck {
    AccountCredentialIssuanceRecheck::new(
        identifier(ACTOR),
        SessionTokenHash::from_bytes([ACTOR_SESSION; SESSION_DIGEST_LENGTH]).unwrap(),
        Name::new("web-ui").unwrap(),
        CredentialRevision::INITIAL,
        SessionInstant::from_unix_milliseconds(NOW).unwrap(),
        AccountCredentialIssuanceFactor::Totp {
            target: target(),
            factor: identifier(0x61),
            verified_step: MfaTimeStep::from_step(step).unwrap(),
        },
    )
}

fn create_mutation(
    surface: &Surface,
    byte: u8,
    username: &str,
    recheck: AccountCredentialIssuanceRecheck,
) -> AccountCreateMutation {
    let account = identifier(byte);
    AccountCreateMutation::new(
        recheck,
        Account {
            identifier: account,
            username: Name::new(username).unwrap(),
            display_name: Some(Name::new(format!("User {byte}")).unwrap()),
            active: true,
            mfa_required: false,
            credential_revision: CredentialRevision::INITIAL,
            must_change_password: true,
            temporary_credential_expiration: Some(
                TemporaryCredentialExpiration::from_unix_milliseconds(EXPIRATION).unwrap(),
            ),
        },
        AccountPublicIdentity::new(
            account,
            public_identifier(&surface.public_identifiers, byte + 0x20),
        ),
        AccountAuditReference::new(
            account,
            audit_reference(&surface.audit_references, byte + 0x40),
        ),
        AccountPasswordVerifier {
            account,
            verifier: PasswordVerifier::new(format!("$verifier-{byte}")).unwrap(),
        },
    )
    .unwrap()
}

fn reset_mutation(
    surface: &mut Surface,
    recheck: AccountCredentialIssuanceRecheck,
    verifier: &str,
    expiration: i64,
) -> AccountPasswordResetMutation {
    let target = surface
        .database
        .prepare_password_reset_target(
            &surface.public_identifiers,
            &surface.audit_references,
            public_identifier(&surface.public_identifiers, TARGET + 0x20),
        )
        .unwrap()
        .unwrap();
    AccountPasswordResetMutation::new(
        recheck,
        target,
        TemporaryCredentialExpiration::from_unix_milliseconds(expiration).unwrap(),
        AccountPasswordVerifier {
            account: identifier(TARGET),
            verifier: PasswordVerifier::new(verifier).unwrap(),
        },
    )
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
        conflict: terminal(persistence, base + 1, base + 1),
        denied: terminal(persistence, base + 2, base + 2),
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

fn account_exists(path: &PathBuf, account: u8) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM weavelit_account WHERE account_id = ?1)",
            [identifier(account).as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .unwrap()
        != 0
}

fn session_count(path: &PathBuf, account: u8) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM weavelit_session WHERE account_id = ?1",
            [identifier(account).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .unwrap()
}

fn insert_factor(path: &PathBuf, enabled: bool) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_mfa_factor \
             (factor_id, account_id, module, protected_factor_data) \
             VALUES (?1, ?2, 'totp', X'010203')",
            params![
                identifier(0x61).as_bytes().as_slice(),
                identifier(ACTOR).as_bytes().as_slice(),
            ],
        )
        .unwrap();
    if enabled {
        connection
            .execute(
                "INSERT INTO weavelit_configuration \
                 (component, setting_key, setting_value) \
                 VALUES ('totp', 'mfa-module.enabled', ?1)",
                [COMPONENT_ENABLED_VALUE],
            )
            .unwrap();
    }
}

fn watermark(path: &PathBuf) -> Option<i64> {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT accepted_step FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1",
            [identifier(0x61).as_bytes().as_slice()],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}

#[test]
fn create_commits_fixed_state_and_selected_terminal_across_restart() {
    let mut surface = surface(false);
    let mutation = create_mutation(&surface, 10, "created", recheck(ACTOR_SESSION));
    let terminals = terminals(&surface.recovery, 0x81);

    assert_eq!(
        surface.database.create_account(
            &surface.public_identifiers,
            &mutation,
            &terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Created)
    );
    let stored: StoredAccountCredentialState = Connection::open(&surface.path)
        .unwrap()
        .query_row(
            "SELECT account.username, account.display_name, account.active, \
                 account.mfa_required, account.credential_revision, account.must_change_password, \
                 account.temporary_credential_expires_at_milliseconds, verifier.encoded_verifier \
                 FROM weavelit_account AS account JOIN weavelit_password_verifier AS verifier \
                   ON verifier.account_id = account.account_id WHERE account.account_id = ?1",
            [identifier(10).as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(stored.0, "created");
    assert_eq!(stored.1.as_deref(), Some("User 10"));
    assert_eq!((stored.2, stored.3, stored.5), (1, 0, 1));
    assert_eq!(stored.4, CredentialRevision::INITIAL.to_stored_bytes());
    assert_eq!(stored.6, Some(EXPIRATION));
    assert_eq!(stored.7, "$verifier-10");
    assert_eq!(session_count(&surface.path, 10), 0);
    assert_eq!(pending_identifiers(&mut surface), [[0x81; 16]]);

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    assert!(account_exists(&surface.path, 10));
    assert_eq!(pending_identifiers(&mut surface), [[0x81; 16]]);
    assert!(
        surface
            .database
            .prepare_password_reset_target(
                &surface.public_identifiers,
                &surface.audit_references,
                public_identifier(&surface.public_identifiers, 0x2a),
            )
            .unwrap()
            .is_some()
    );
}

#[test]
fn create_conflict_and_exact_session_denial_select_only_their_terminal() {
    let mut duplicate = surface(false);
    let mutation = create_mutation(&duplicate, 10, "actor", recheck(ACTOR_SESSION));
    let duplicate_terminals = terminals(&duplicate.recovery, 0x84);
    assert_eq!(
        duplicate.database.create_account(
            &duplicate.public_identifiers,
            &mutation,
            &duplicate_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Conflict)
    );
    assert!(!account_exists(&duplicate.path, 10));
    assert_eq!(pending_identifiers(&mut duplicate), [[0x85; 16]]);

    let mut denied = surface(false);
    let mutation = create_mutation(&denied, 11, "denied", recheck(0x7f));
    let denied_terminals = terminals(&denied.recovery, 0x87);
    assert_eq!(
        denied.database.create_account(
            &denied.public_identifiers,
            &mutation,
            &denied_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Denied)
    );
    assert!(!account_exists(&denied.path, 11));
    assert_eq!(pending_identifiers(&mut denied), [[0x89; 16]]);
}

#[test]
fn reset_preserves_target_state_replaces_credential_and_revokes_every_target_session() {
    let mut surface = surface(true);
    insert_session(&surface.path, 0x41, TARGET, "web-ui", 1_000);
    insert_session(&surface.path, 0x42, TARGET, "other-client", 1_000);
    let mutation = reset_mutation(
        &mut surface,
        recheck(ACTOR_SESSION),
        "$reset-verifier",
        EXPIRATION,
    );
    let terminals = terminals(&surface.recovery, 0x91);

    assert_eq!(
        surface.database.reset_account_password(
            &surface.public_identifiers,
            &mutation,
            &terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Reset {
            revoked_sessions: 2
        })
    );
    let stored: StoredAccountCredentialState = Connection::open(&surface.path)
        .unwrap()
        .query_row(
            "SELECT account.username, account.display_name, account.active, \
                 account.mfa_required, account.credential_revision, account.must_change_password, \
                 account.temporary_credential_expires_at_milliseconds, verifier.encoded_verifier \
                 FROM weavelit_account AS account JOIN weavelit_password_verifier AS verifier \
                   ON verifier.account_id = account.account_id WHERE account.account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(stored.0, "target");
    assert_eq!(stored.1.as_deref(), Some("Target"));
    assert_eq!((stored.2, stored.3, stored.5), (1, 0, 1));
    assert_eq!(stored.4, 2_u64.to_be_bytes());
    assert_eq!(stored.6, Some(EXPIRATION));
    assert_eq!(stored.7, "$reset-verifier");
    assert_eq!(session_count(&surface.path, TARGET), 0);
    assert_eq!(session_count(&surface.path, ACTOR), 1);
    assert_eq!(pending_identifiers(&mut surface), [[0x91; 16]]);
}

#[test]
fn self_reset_validates_then_revokes_its_own_issuer_session() {
    let mut surface = surface(false);
    let target = surface
        .database
        .prepare_password_reset_target(
            &surface.public_identifiers,
            &surface.audit_references,
            public_identifier(&surface.public_identifiers, ACTOR + 0x20),
        )
        .unwrap()
        .unwrap();
    let mutation = AccountPasswordResetMutation::new(
        recheck(ACTOR_SESSION),
        target,
        TemporaryCredentialExpiration::from_unix_milliseconds(EXPIRATION).unwrap(),
        AccountPasswordVerifier {
            account: identifier(ACTOR),
            verifier: PasswordVerifier::new("$self-reset").unwrap(),
        },
    )
    .unwrap();
    let terminals = terminals(&surface.recovery, 0x94);

    assert_eq!(
        surface.database.reset_account_password(
            &surface.public_identifiers,
            &mutation,
            &terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Reset {
            revoked_sessions: 1
        })
    );
    assert_eq!(session_count(&surface.path, ACTOR), 0);
    assert_eq!(pending_identifiers(&mut surface), [[0x94; 16]]);
}

#[test]
fn stale_and_explicit_sequential_resets_have_distinct_outcomes() {
    let mut stale = surface(true);
    insert_session(&stale.path, 0x41, TARGET, "web-ui", 1_000);
    let mutation = reset_mutation(
        &mut stale,
        recheck(ACTOR_SESSION),
        "$not-written",
        EXPIRATION,
    );
    Connection::open(&stale.path)
        .unwrap()
        .execute(
            "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
            params![
                identifier(TARGET).as_bytes().as_slice(),
                2_u64.to_be_bytes().as_slice()
            ],
        )
        .unwrap();
    let stale_terminals = terminals(&stale.recovery, 0x97);
    assert_eq!(
        stale.database.reset_account_password(
            &stale.public_identifiers,
            &mutation,
            &stale_terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Stale)
    );
    assert_eq!(session_count(&stale.path, TARGET), 1);
    assert_eq!(pending_identifiers(&mut stale), [[0x98; 16]]);

    let mut sequential = surface(true);
    let first = reset_mutation(
        &mut sequential,
        recheck(ACTOR_SESSION),
        "$first-reset",
        EXPIRATION,
    );
    let first_terminals = terminals(&sequential.recovery, 0xa1);
    assert!(matches!(
        sequential.database.reset_account_password(
            &sequential.public_identifiers,
            &first,
            &first_terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Reset { .. })
    ));
    let second = reset_mutation(
        &mut sequential,
        recheck(ACTOR_SESSION),
        "$second-reset",
        EXPIRATION + 1,
    );
    let second_terminals = terminals(&sequential.recovery, 0xa4);
    assert!(matches!(
        sequential.database.reset_account_password(
            &sequential.public_identifiers,
            &second,
            &second_terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Reset { .. })
    ));
    let stored: (Vec<u8>, String, i64) = Connection::open(&sequential.path)
        .unwrap()
        .query_row(
            "SELECT account.credential_revision, verifier.encoded_verifier, \
             account.temporary_credential_expires_at_milliseconds \
             FROM weavelit_account AS account JOIN weavelit_password_verifier AS verifier \
               ON verifier.account_id = account.account_id WHERE account.account_id = ?1",
            [identifier(TARGET).as_bytes().as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        stored,
        (
            3_u64.to_be_bytes().to_vec(),
            "$second-reset".to_owned(),
            EXPIRATION + 1
        )
    );
}

#[test]
fn final_session_revision_and_lifetime_mismatches_deny_without_business_state() {
    let cases = [
        (
            "wrong Client Module",
            ACTOR_SESSION,
            "other",
            NOW,
            CredentialRevision::INITIAL,
        ),
        (
            "expired session",
            ACTOR_SESSION,
            "web-ui",
            1_000 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            CredentialRevision::INITIAL,
        ),
        (
            "stale actor revision",
            ACTOR_SESSION,
            "web-ui",
            NOW,
            CredentialRevision::from_value(2).unwrap(),
        ),
    ];
    for (label, session, client_module, now, revision) in cases {
        let mut surface = surface(false);
        let mutation = create_mutation(
            &surface,
            12,
            "must-not-exist",
            recheck_at(session, ACTOR, client_module, now, revision),
        );
        let terminals = terminals(&surface.recovery, 0xaa);
        assert_eq!(
            surface.database.create_account(
                &surface.public_identifiers,
                &mutation,
                &terminals.writes(),
            ),
            Ok(AccountCreateOutcome::Denied),
            "{label}"
        );
        assert!(!account_exists(&surface.path, 12), "{label}");
    }

    let mut revoked = surface(false);
    Connection::open(&revoked.path)
        .unwrap()
        .execute("DELETE FROM weavelit_session", [])
        .unwrap();
    let mutation = create_mutation(&revoked, 12, "must-not-exist", recheck(ACTOR_SESSION));
    let terminals = terminals(&revoked.recovery, 0xad);
    assert_eq!(
        revoked.database.create_account(
            &revoked.public_identifiers,
            &mutation,
            &terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Denied)
    );
}

#[test]
fn totp_replay_disabled_module_and_enrollment_race_deny_uniformly() {
    let mut replay = surface(false);
    insert_factor(&replay.path, true);
    let first = create_mutation(&replay, 13, "first-totp", totp_recheck(10));
    let first_terminals = terminals(&replay.recovery, 0xb1);
    assert_eq!(
        replay.database.create_account(
            &replay.public_identifiers,
            &first,
            &first_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Created)
    );
    let repeated = create_mutation(&replay, 14, "replayed-totp", totp_recheck(10));
    let repeated_terminals = terminals(&replay.recovery, 0xb4);
    assert_eq!(
        replay.database.create_account(
            &replay.public_identifiers,
            &repeated,
            &repeated_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Denied)
    );
    assert!(!account_exists(&replay.path, 14));

    let mut disabled = surface(false);
    insert_factor(&disabled.path, false);
    let mutation = create_mutation(&disabled, 15, "disabled-totp", totp_recheck(11));
    let disabled_terminals = terminals(&disabled.recovery, 0xb7);
    assert_eq!(
        disabled.database.create_account(
            &disabled.public_identifiers,
            &mutation,
            &disabled_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Denied)
    );

    let mut enrollment_race = surface(false);
    let mutation = create_mutation(
        &enrollment_race,
        16,
        "enrollment-race",
        recheck(ACTOR_SESSION),
    );
    insert_factor(&enrollment_race.path, true);
    let race_terminals = terminals(&enrollment_race.recovery, 0xba);
    assert_eq!(
        enrollment_race.database.create_account(
            &enrollment_race.public_identifiers,
            &mutation,
            &race_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Denied)
    );

    assert_eq!(format!("{:?}", AccountCreateOutcome::Denied), "Denied");
    assert!(!account_exists(&disabled.path, 15));
    assert!(!account_exists(&enrollment_race.path, 16));
}

#[test]
fn create_conflict_and_stale_reset_do_not_advance_the_totp_watermark() {
    let mut conflict = surface(false);
    insert_factor(&conflict.path, true);
    let mutation = create_mutation(&conflict, 17, "actor", totp_recheck(10));
    let conflict_terminals = terminals(&conflict.recovery, 0xbd);
    assert_eq!(
        conflict.database.create_account(
            &conflict.public_identifiers,
            &mutation,
            &conflict_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Conflict)
    );
    assert_eq!(watermark(&conflict.path), None);

    let mut stale = surface(true);
    insert_factor(&stale.path, true);
    let mutation = reset_mutation(&mut stale, totp_recheck(11), "$not-written", EXPIRATION);
    Connection::open(&stale.path)
        .unwrap()
        .execute(
            "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
            params![
                identifier(TARGET).as_bytes().as_slice(),
                2_u64.to_be_bytes().as_slice(),
            ],
        )
        .unwrap();
    let stale_terminals = terminals(&stale.recovery, 0xc0);
    assert_eq!(
        stale.database.reset_account_password(
            &stale.public_identifiers,
            &mutation,
            &stale_terminals.writes(),
        ),
        Ok(AccountPasswordResetOutcome::Stale)
    );
    assert_eq!(watermark(&stale.path), None);
}

#[test]
fn terminal_persistence_failure_rolls_back_business_state() {
    let mut surface = surface(false);
    insert_factor(&surface.path, true);
    let first = create_mutation(&surface, 17, "first", totp_recheck(10));
    let first_terminals = Terminals {
        succeeded: terminal(&surface.recovery, 0xc1, 0x11),
        conflict: terminal(&surface.recovery, 0xc2, 0x12),
        denied: terminal(&surface.recovery, 0xc3, 0x13),
    };
    assert_eq!(
        surface.database.create_account(
            &surface.public_identifiers,
            &first,
            &first_terminals.writes(),
        ),
        Ok(AccountCreateOutcome::Created)
    );
    assert_eq!(watermark(&surface.path), Some(10));

    let second = create_mutation(&surface, 18, "second", totp_recheck(11));
    let colliding = Terminals {
        succeeded: terminal(&surface.recovery, 0xc1, 0x22),
        conflict: terminal(&surface.recovery, 0xc4, 0x23),
        denied: terminal(&surface.recovery, 0xc5, 0x24),
    };
    assert_eq!(
        surface
            .database
            .create_account(&surface.public_identifiers, &second, &colliding.writes(),),
        Err(DatabaseError::InvalidState)
    );
    assert!(!account_exists(&surface.path, 18));
    assert_eq!(watermark(&surface.path), Some(10));
    assert_eq!(pending_identifiers(&mut surface), [[0xc1; 16]]);
}

#[test]
fn competing_creates_and_resets_commit_one_success_each() {
    let creates = surface(false);
    let create_barrier = Arc::new(Barrier::new(2));
    let mut create_threads = Vec::new();
    for (byte, base) in [(20, 0xd1), (21, 0xd4)] {
        let path = creates.path.clone();
        let barrier = Arc::clone(&create_barrier);
        let public_identifiers = creates.public_identifiers;
        let audit_references = creates.audit_references;
        let recovery = AuditTerminalRecoveryPersistence::from_server_authority(
            &ServerDatabaseAuthority::new(),
        );
        create_threads.push(std::thread::spawn(move || {
            let mut database = SqliteDatabase::open(&path).unwrap();
            let temporary_surface = Surface {
                _directory: tempfile::tempdir().unwrap(),
                path: path.clone(),
                database: SqliteDatabase::open(&path).unwrap(),
                public_identifiers,
                audit_references,
                recovery,
            };
            let mutation = create_mutation(
                &temporary_surface,
                byte,
                "concurrent",
                recheck(ACTOR_SESSION),
            );
            let terminals = terminals(&temporary_surface.recovery, base);
            drop(temporary_surface.database);
            barrier.wait();
            database
                .create_account(&public_identifiers, &mutation, &terminals.writes())
                .unwrap()
        }));
    }
    let mut create_outcomes = create_threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    create_outcomes.sort_by_key(|outcome| match outcome {
        AccountCreateOutcome::Created => 0,
        AccountCreateOutcome::Conflict => 1,
        AccountCreateOutcome::Denied => 2,
    });
    assert_eq!(
        create_outcomes,
        [
            AccountCreateOutcome::Created,
            AccountCreateOutcome::Conflict
        ]
    );
    assert_eq!(
        Connection::open(&creates.path)
            .unwrap()
            .query_row(
                "SELECT count(*) FROM weavelit_account WHERE username = 'concurrent'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
        1
    );

    let mut resets = surface(true);
    let first_target = resets
        .database
        .prepare_password_reset_target(
            &resets.public_identifiers,
            &resets.audit_references,
            public_identifier(&resets.public_identifiers, TARGET + 0x20),
        )
        .unwrap()
        .unwrap();
    let second_target = resets
        .database
        .prepare_password_reset_target(
            &resets.public_identifiers,
            &resets.audit_references,
            public_identifier(&resets.public_identifiers, TARGET + 0x20),
        )
        .unwrap()
        .unwrap();
    let reset_barrier = Arc::new(Barrier::new(2));
    let mut reset_threads = Vec::new();
    for (target, verifier, base) in [
        (first_target, "$concurrent-one", 0xe1),
        (second_target, "$concurrent-two", 0xe4),
    ] {
        let path = resets.path.clone();
        let barrier = Arc::clone(&reset_barrier);
        let public_identifiers = resets.public_identifiers;
        let recovery = AuditTerminalRecoveryPersistence::from_server_authority(
            &ServerDatabaseAuthority::new(),
        );
        reset_threads.push(std::thread::spawn(move || {
            let mutation = AccountPasswordResetMutation::new(
                recheck(ACTOR_SESSION),
                target,
                TemporaryCredentialExpiration::from_unix_milliseconds(EXPIRATION).unwrap(),
                AccountPasswordVerifier {
                    account: identifier(TARGET),
                    verifier: PasswordVerifier::new(verifier).unwrap(),
                },
            )
            .unwrap();
            let terminals = terminals(&recovery, base);
            let mut database = SqliteDatabase::open(&path).unwrap();
            barrier.wait();
            database
                .reset_account_password(&public_identifiers, &mutation, &terminals.writes())
                .unwrap()
        }));
    }
    let mut reset_outcomes = reset_threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect::<Vec<_>>();
    reset_outcomes.sort_by_key(|outcome| match outcome {
        AccountPasswordResetOutcome::Reset { .. } => 0,
        AccountPasswordResetOutcome::Stale => 1,
        AccountPasswordResetOutcome::Denied => 2,
    });
    assert!(matches!(
        reset_outcomes[0],
        AccountPasswordResetOutcome::Reset { .. }
    ));
    assert_eq!(reset_outcomes[1], AccountPasswordResetOutcome::Stale);
}

#[test]
fn application_database_trait_exposes_the_writer_store() {
    let mut surface = surface(false);
    let database: &mut dyn ApplicationDatabase = &mut surface.database;
    assert!(database.account_credential_writers().is_some());
}
