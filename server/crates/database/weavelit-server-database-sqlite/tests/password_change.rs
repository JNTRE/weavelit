use std::{
    path::PathBuf,
    sync::{Arc, Barrier},
};

use rusqlite::{Connection, params};
use tempfile::TempDir;
use weavelit_server_database::{
    AccountPasswordVerifier, AuditTerminalRecoveryPersistence, AuditTerminalRecoveryStore,
    AuditTerminalReplayBatchSize, CredentialRevision, DatabaseError, Name, NewSession,
    PasswordChangeAuditTerminalWrites, PasswordChangeMutation, PasswordChangeOutcome,
    PasswordChangeRecheck, PasswordChangeWriterStore, PasswordVerifier,
    SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH, SessionCsrfHash, SessionInstant,
    SessionPosture, SessionStore, SessionTokenHash, SessionValidation, StateIdentifier,
    StoredAuditDestinationBinding, ValidatedAuditTerminalObligationWrite,
};
use weavelit_server_database_authority::ServerDatabaseAuthority;
use weavelit_server_database_sqlite::SqliteDatabase;

const ACCOUNT: u8 = 0x21;
const CURRENT_SESSION: u8 = 0x31;
const OTHER_SESSION: u8 = 0x32;
const FRESH_SESSION: u8 = 0x41;
const NOW: i64 = 10_000;
const EXPIRATION: i64 = NOW + 60_000;
const CURRENT_VERIFIER: &str = "$current-verifier";
const REPLACEMENT_VERIFIER: &str = "$replacement-verifier";

struct Surface {
    _directory: TempDir,
    path: PathBuf,
    database: SqliteDatabase,
    recovery: AuditTerminalRecoveryPersistence,
}

struct Terminals {
    succeeded: ValidatedAuditTerminalObligationWrite,
    denied: ValidatedAuditTerminalObligationWrite,
}

impl Terminals {
    fn writes(&self) -> PasswordChangeAuditTerminalWrites<'_> {
        PasswordChangeAuditTerminalWrites::new(&self.succeeded, &self.denied)
    }
}

fn identifier(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn instant(value: i64) -> SessionInstant {
    SessionInstant::from_unix_milliseconds(value).unwrap()
}

fn token(byte: u8) -> SessionTokenHash {
    SessionTokenHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap()
}

fn csrf(byte: u8) -> SessionCsrfHash {
    SessionCsrfHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap()
}

fn surface() -> Surface {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db");
    let database = SqliteDatabase::open(&path).unwrap();
    insert_account(
        &path,
        ACCOUNT,
        true,
        1,
        true,
        Some(EXPIRATION),
        CURRENT_VERIFIER,
    );
    insert_session(&path, CURRENT_SESSION, ACCOUNT, "web-ui", NOW - 1);
    insert_session(&path, OTHER_SESSION, ACCOUNT, "cli", NOW - 1);
    let recovery =
        AuditTerminalRecoveryPersistence::from_server_authority(&ServerDatabaseAuthority::new());
    Surface {
        _directory: directory,
        path,
        database,
        recovery,
    }
}

fn insert_account(
    path: &PathBuf,
    account: u8,
    active: bool,
    revision: u64,
    must_change: bool,
    expiration: Option<i64>,
    verifier: &str,
) {
    let connection = Connection::open(path).unwrap();
    connection.execute(
        "INSERT INTO weavelit_account (account_id, username, display_name, active, mfa_required, \
         credential_revision, must_change_password, temporary_credential_expires_at_milliseconds) \
         VALUES (?1, ?2, NULL, ?3, 0, ?4, ?5, ?6)",
        params![
            identifier(account).as_bytes().as_slice(),
            format!("user-{account}"),
            i64::from(active),
            revision.to_be_bytes().as_slice(),
            i64::from(must_change),
            expiration,
        ],
    ).unwrap();
    connection
        .execute(
            "INSERT INTO weavelit_password_verifier (account_id, encoded_verifier) VALUES (?1, ?2)",
            params![identifier(account).as_bytes().as_slice(), verifier],
        )
        .unwrap();
}

fn insert_session(path: &PathBuf, digest: u8, account: u8, client: &str, issued: i64) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO weavelit_session (token_hash, csrf_hash, account_id, client_module, \
         issued_at_milliseconds, last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)",
            params![
                [digest; SESSION_DIGEST_LENGTH].as_slice(),
                [digest.wrapping_add(1); SESSION_DIGEST_LENGTH].as_slice(),
                identifier(account).as_bytes().as_slice(),
                client,
                issued,
                issued + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
            ],
        )
        .unwrap();
}

fn mutation(session: u8, fresh: u8, now: i64) -> PasswordChangeMutation {
    PasswordChangeMutation::new(
        PasswordChangeRecheck::new(
            identifier(ACCOUNT),
            token(session),
            Name::new("web-ui").unwrap(),
            CredentialRevision::INITIAL,
            PasswordVerifier::new(CURRENT_VERIFIER).unwrap(),
            instant(now),
        ),
        AccountPasswordVerifier {
            account: identifier(ACCOUNT),
            verifier: PasswordVerifier::new(REPLACEMENT_VERIFIER).unwrap(),
        },
        NewSession::new(
            token(fresh),
            csrf(fresh.wrapping_add(1)),
            identifier(ACCOUNT),
            CredentialRevision::from_value(2).unwrap(),
            Name::new("web-ui").unwrap(),
            instant(now),
        ),
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
        denied: terminal(persistence, base + 1, base + 1),
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
        .map(|entry| *entry.identifier().as_bytes())
        .collect()
}

fn credential_state(path: &PathBuf) -> (i64, Vec<u8>, i64, Option<i64>, String) {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT account.active, account.credential_revision, account.must_change_password, \
         account.temporary_credential_expires_at_milliseconds, verifier.encoded_verifier \
         FROM weavelit_account AS account JOIN weavelit_password_verifier AS verifier \
         ON verifier.account_id = account.account_id WHERE account.account_id = ?1",
            [identifier(ACCOUNT).as_bytes().as_slice()],
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
        .unwrap()
}

fn sessions(path: &PathBuf) -> Vec<Vec<u8>> {
    let connection = Connection::open(path).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT token_hash FROM weavelit_session WHERE account_id = ?1 ORDER BY token_hash",
        )
        .unwrap();
    statement
        .query_map([identifier(ACCOUNT).as_bytes().as_slice()], |row| {
            row.get(0)
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn successful_change_replaces_credential_and_every_session_atomically_across_restart() {
    let mut surface = surface();
    let terminals = terminals(&surface.recovery, 0x81);

    assert_eq!(
        surface.database.change_password(
            &mutation(CURRENT_SESSION, FRESH_SESSION, NOW),
            &terminals.writes()
        ),
        Ok(PasswordChangeOutcome::Changed {
            revoked_sessions: 2
        })
    );
    assert_eq!(
        credential_state(&surface.path),
        (
            1,
            2_u64.to_be_bytes().to_vec(),
            0,
            None,
            REPLACEMENT_VERIFIER.to_owned()
        )
    );
    assert_eq!(
        sessions(&surface.path),
        vec![vec![FRESH_SESSION; SESSION_DIGEST_LENGTH]]
    );
    assert_eq!(pending(&mut surface), [[0x81; 16]]);

    drop(surface.database);
    surface.database = SqliteDatabase::open(&surface.path).unwrap();
    let SessionValidation::Valid(fresh) = surface
        .database
        .validate_and_touch(
            &token(FRESH_SESSION),
            &csrf(FRESH_SESSION + 1),
            instant(NOW + 1),
        )
        .unwrap()
    else {
        panic!("the fresh session must survive restart")
    };
    assert_eq!(fresh.posture(), SessionPosture::Ordinary);
}

fn denied_case(label: &str, race: impl FnOnce(&PathBuf), now: i64) {
    let mut surface = surface();
    let prepared = mutation(CURRENT_SESSION, FRESH_SESSION, now);
    race(&surface.path);
    let expected_state = credential_state(&surface.path);
    let expected_sessions = sessions(&surface.path);
    let terminals = terminals(&surface.recovery, 0x91);

    assert_eq!(
        surface
            .database
            .change_password(&prepared, &terminals.writes()),
        Ok(PasswordChangeOutcome::Denied),
        "{label}"
    );
    assert_eq!(credential_state(&surface.path), expected_state, "{label}");
    assert_eq!(sessions(&surface.path), expected_sessions, "{label}");
    assert_eq!(pending(&mut surface), [[0x92; 16]], "{label}");
}

#[test]
fn revoked_stale_expired_reset_disabled_and_completed_races_select_denied_only() {
    denied_case(
        "revoked session",
        |path| {
            Connection::open(path)
                .unwrap()
                .execute(
                    "DELETE FROM weavelit_session WHERE token_hash = ?1",
                    [[CURRENT_SESSION; SESSION_DIGEST_LENGTH].as_slice()],
                )
                .unwrap();
        },
        NOW,
    );
    denied_case(
        "stale revision and reset verifier",
        |path| {
            let connection = Connection::open(path).unwrap();
            connection
                .execute(
                    "UPDATE weavelit_account SET credential_revision = ?2 WHERE account_id = ?1",
                    params![
                        identifier(ACCOUNT).as_bytes().as_slice(),
                        2_u64.to_be_bytes().as_slice()
                    ],
                )
                .unwrap();
            connection.execute(
            "UPDATE weavelit_password_verifier SET encoded_verifier = '$reset' WHERE account_id = ?1",
            [identifier(ACCOUNT).as_bytes().as_slice()],
        ).unwrap();
        },
        NOW,
    );
    denied_case(
        "expired temporary credential",
        |path| {
            Connection::open(path)
                .unwrap()
                .execute(
                    "UPDATE weavelit_account SET temporary_credential_expires_at_milliseconds = ?2 \
             WHERE account_id = ?1",
                    params![identifier(ACCOUNT).as_bytes().as_slice(), NOW],
                )
                .unwrap();
        },
        NOW,
    );
    denied_case(
        "disabled account",
        |path| {
            Connection::open(path).unwrap().execute(
            "UPDATE weavelit_account SET active = 0, credential_revision = ?2 WHERE account_id = ?1",
            params![identifier(ACCOUNT).as_bytes().as_slice(), 2_u64.to_be_bytes().as_slice()],
        ).unwrap();
        },
        NOW,
    );
    denied_case(
        "another completed change",
        |path| {
            let connection = Connection::open(path).unwrap();
            connection.execute(
            "UPDATE weavelit_account SET credential_revision = ?2, must_change_password = 0, \
             temporary_credential_expires_at_milliseconds = NULL WHERE account_id = ?1",
            params![identifier(ACCOUNT).as_bytes().as_slice(), 2_u64.to_be_bytes().as_slice()],
        ).unwrap();
            connection.execute(
            "UPDATE weavelit_password_verifier SET encoded_verifier = '$other-change' WHERE account_id = ?1",
            [identifier(ACCOUNT).as_bytes().as_slice()],
        ).unwrap();
        },
        NOW,
    );
    denied_case(
        "expired exact session",
        |_| {},
        NOW - 1 + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS,
    );
}

#[test]
fn audit_terminal_and_fresh_session_collisions_roll_back_every_business_write() {
    let mut audit_collision = surface();
    let first = Terminals {
        succeeded: terminal(&audit_collision.recovery, 0xa0, 0x10),
        denied: terminal(&audit_collision.recovery, 0xa1, 0x11),
    };
    assert_eq!(
        audit_collision
            .database
            .change_password(&mutation(0x7f, 0x51, NOW), &first.writes(),),
        Ok(PasswordChangeOutcome::Denied)
    );
    let before_state = credential_state(&audit_collision.path);
    let before_sessions = sessions(&audit_collision.path);
    let colliding = Terminals {
        succeeded: terminal(&audit_collision.recovery, 0xa1, 0x22),
        denied: terminal(&audit_collision.recovery, 0xa2, 0x23),
    };
    assert_eq!(
        audit_collision.database.change_password(
            &mutation(CURRENT_SESSION, FRESH_SESSION, NOW),
            &colliding.writes(),
        ),
        Err(DatabaseError::InvalidState)
    );
    assert_eq!(credential_state(&audit_collision.path), before_state);
    assert_eq!(sessions(&audit_collision.path), before_sessions);
    assert_eq!(pending(&mut audit_collision), [[0xa1; 16]]);

    let mut session_collision = surface();
    insert_account(
        &session_collision.path,
        0x55,
        true,
        1,
        false,
        None,
        "$other",
    );
    insert_session(
        &session_collision.path,
        FRESH_SESSION,
        0x55,
        "web-ui",
        NOW - 1,
    );
    let before_state = credential_state(&session_collision.path);
    let before_sessions = sessions(&session_collision.path);
    let terminals = terminals(&session_collision.recovery, 0xb1);
    assert_eq!(
        session_collision.database.change_password(
            &mutation(CURRENT_SESSION, FRESH_SESSION, NOW),
            &terminals.writes(),
        ),
        Err(DatabaseError::IntegrityFailure)
    );
    assert_eq!(credential_state(&session_collision.path), before_state);
    assert_eq!(sessions(&session_collision.path), before_sessions);
    assert!(pending(&mut session_collision).is_empty());
}

#[test]
fn concurrent_changes_commit_exactly_one_success_and_one_denial() {
    let surface = surface();
    let path = surface.path.clone();
    let barrier = Arc::new(Barrier::new(2));
    let mut threads = Vec::new();
    for (fresh, base) in [(0x61, 0xc1), (0x62, 0xc3)] {
        let path = path.clone();
        let barrier = Arc::clone(&barrier);
        threads.push(std::thread::spawn(move || {
            let mut database = SqliteDatabase::open(&path).unwrap();
            let recovery = AuditTerminalRecoveryPersistence::from_server_authority(
                &ServerDatabaseAuthority::new(),
            );
            let terminals = terminals(&recovery, base);
            barrier.wait();
            database
                .change_password(&mutation(CURRENT_SESSION, fresh, NOW), &terminals.writes())
                .unwrap()
        }));
    }
    let outcomes: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| matches!(outcome, PasswordChangeOutcome::Changed { .. }))
            .count(),
        1
    );
    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| **outcome == PasswordChangeOutcome::Denied)
            .count(),
        1
    );
    assert_eq!(credential_state(&path).1, 2_u64.to_be_bytes());
    assert_eq!(sessions(&path).len(), 1);
}
