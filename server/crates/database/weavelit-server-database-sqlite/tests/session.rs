//! Live session storage, lifetime enforcement, and restart survival.
//!
//! Every instant is injected, so no assertion depends on wall-clock timing or
//! on a sleep.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;
use weavelit_server_database::{
    Name, NewSession, SESSION_ABSOLUTE_LIFETIME_MILLISECONDS, SESSION_DIGEST_LENGTH,
    SESSION_IDLE_TIMEOUT_MILLISECONDS, SessionCsrfHash, SessionInstant, SessionRejection,
    SessionStore, SessionTokenHash, SessionValidation, StateIdentifier, StoredSession,
};
use weavelit_server_database_sqlite::SqliteDatabase;

const ISSUED_AT: i64 = 10_000;
const TOKEN_BYTE: u8 = 0x31;
const CSRF_BYTE: u8 = 0x32;

fn database_path(temporary_directory: &TempDir) -> PathBuf {
    temporary_directory
        .path()
        .canonicalize()
        .unwrap()
        .join("application.db")
}

fn token(byte: u8) -> SessionTokenHash {
    SessionTokenHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap()
}

fn csrf(byte: u8) -> SessionCsrfHash {
    SessionCsrfHash::from_bytes([byte; SESSION_DIGEST_LENGTH]).unwrap()
}

fn account(byte: u8) -> StateIdentifier {
    StateIdentifier::from_bytes([byte; 16]).unwrap()
}

fn instant(value: i64) -> SessionInstant {
    SessionInstant::from_unix_milliseconds(value).unwrap()
}

fn new_session(token_byte: u8, account_byte: u8) -> NewSession {
    NewSession::new(
        token(token_byte),
        csrf(CSRF_BYTE),
        account(account_byte),
        Name::new("web-ui").unwrap(),
        instant(ISSUED_AT),
    )
}

fn opened(path: &Path) -> SqliteDatabase {
    SqliteDatabase::open(path).unwrap()
}

fn stored(validation: SessionValidation) -> StoredSession {
    match validation {
        SessionValidation::Valid(session) => session,
        SessionValidation::Rejected(rejection) => {
            panic!("the session must be valid, but it was rejected as {rejection:?}")
        }
    }
}

fn rejection(validation: SessionValidation) -> SessionRejection {
    match validation {
        SessionValidation::Valid(_) => panic!("the session must be rejected"),
        SessionValidation::Rejected(rejection) => rejection,
    }
}

fn session_count(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("SELECT count(*) FROM weavelit_session", [], |row| {
            row.get(0)
        })
        .unwrap()
}

/// Keeps one session continuously active up to `target` without ever letting
/// the idle window lapse, so only the absolute lifetime can end it.
fn keep_active_until(database: &mut SqliteDatabase, token_hash: &SessionTokenHash, target: i64) {
    let mut now = ISSUED_AT;
    while now + SESSION_IDLE_TIMEOUT_MILLISECONDS <= target {
        now += SESSION_IDLE_TIMEOUT_MILLISECONDS - 1;
        stored(
            database
                .validate_and_touch(token_hash, &csrf(CSRF_BYTE), instant(now))
                .unwrap(),
        );
    }
    if now < target {
        stored(
            database
                .validate_and_touch(token_hash, &csrf(CSRF_BYTE), instant(target))
                .unwrap(),
        );
    }
}

#[test]
fn a_stored_session_survives_an_ordinary_restart() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    drop(database);

    let mut reopened = opened(&path);
    let session = stored(
        reopened
            .validate_and_touch(&token(TOKEN_BYTE), &csrf(CSRF_BYTE), instant(ISSUED_AT + 1))
            .unwrap(),
    );

    assert_eq!(session.account(), account(0x41));
    assert_eq!(session.client_module().as_str(), "web-ui");
    assert_eq!(session.issued_at(), instant(ISSUED_AT));
    assert_eq!(session.last_seen_at(), instant(ISSUED_AT + 1));
    assert_eq!(
        session.absolute_expires_at(),
        instant(ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS)
    );
}

#[test]
fn an_unknown_token_is_rejected_without_creating_anything() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);

    let validation = database
        .validate_and_touch(&token(0x55), &csrf(CSRF_BYTE), instant(ISSUED_AT))
        .unwrap();

    assert_eq!(rejection(validation), SessionRejection::Unknown);
    assert_eq!(session_count(&path), 0);
}

#[test]
fn a_wrong_csrf_digest_is_rejected_as_unknown_without_advancing_the_activity() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    let idle_deadline = ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS;

    let refused = database
        .validate_and_touch(&token(TOKEN_BYTE), &csrf(0x7A), instant(idle_deadline - 1))
        .unwrap();
    let expired = database
        .validate_and_touch(&token(TOKEN_BYTE), &csrf(CSRF_BYTE), instant(idle_deadline))
        .unwrap();

    assert_eq!(
        rejection(refused),
        SessionRejection::Unknown,
        "a wrong CSRF digest must produce the same rejection an unknown token does"
    );
    assert_eq!(
        rejection(expired),
        SessionRejection::IdleTimeout,
        "the refused request must not have extended the idle deadline"
    );
}

#[test]
fn a_matching_csrf_digest_still_advances_the_activity() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    let idle_deadline = ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS;

    let touched = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(idle_deadline - 1),
        )
        .unwrap();
    let still_live = database
        .validate_and_touch(&token(TOKEN_BYTE), &csrf(CSRF_BYTE), instant(idle_deadline))
        .unwrap();

    assert_eq!(stored(touched).last_seen_at(), instant(idle_deadline - 1));
    assert_eq!(stored(still_live).last_seen_at(), instant(idle_deadline));
}

#[test]
fn the_idle_boundary_is_exact_and_the_expired_session_is_removed() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    let idle_deadline = ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS;

    let last_valid = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(idle_deadline - 1),
        )
        .unwrap();
    let refreshed_deadline = idle_deadline - 1 + SESSION_IDLE_TIMEOUT_MILLISECONDS;
    let expired = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(refreshed_deadline),
        )
        .unwrap();

    assert_eq!(
        stored(last_valid).last_seen_at(),
        instant(idle_deadline - 1)
    );
    assert_eq!(rejection(expired), SessionRejection::IdleTimeout);
    assert_eq!(
        session_count(&path),
        0,
        "an idle-expired session must not remain stored"
    );
}

#[test]
fn the_absolute_boundary_is_exact_and_activity_cannot_extend_it() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    let absolute_deadline = ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS;

    // Continuous activity right up to the absolute deadline.
    keep_active_until(&mut database, &token(TOKEN_BYTE), absolute_deadline - 1);

    let last_valid = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(absolute_deadline - 1),
        )
        .unwrap();
    let expired = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(absolute_deadline),
        )
        .unwrap();

    assert_eq!(
        stored(last_valid).absolute_expires_at(),
        instant(absolute_deadline),
        "activity must never move the absolute expiry"
    );
    assert_eq!(rejection(expired), SessionRejection::AbsoluteLifetime);
    assert_eq!(session_count(&path), 0);
}

#[test]
fn a_backwards_clock_fails_closed_without_touching_or_removing_the_session() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    stored(
        database
            .validate_and_touch(
                &token(TOKEN_BYTE),
                &csrf(CSRF_BYTE),
                instant(ISSUED_AT + 5_000),
            )
            .unwrap(),
    );

    let before_issue = database
        .validate_and_touch(&token(TOKEN_BYTE), &csrf(CSRF_BYTE), instant(ISSUED_AT - 1))
        .unwrap();
    let before_activity = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(ISSUED_AT + 4_999),
        )
        .unwrap();
    let rotation = database
        .rotate_csrf(&token(TOKEN_BYTE), &csrf(0x77), instant(ISSUED_AT - 1))
        .unwrap();
    let recovered = database
        .validate_and_touch(
            &token(TOKEN_BYTE),
            &csrf(CSRF_BYTE),
            instant(ISSUED_AT + 5_001),
        )
        .unwrap();

    assert_eq!(rejection(before_issue), SessionRejection::ClockRollback);
    assert_eq!(rejection(before_activity), SessionRejection::ClockRollback);
    assert_eq!(rejection(rotation), SessionRejection::ClockRollback);
    assert_eq!(
        session_count(&path),
        1,
        "a wrong clock must not destroy a session"
    );
    assert_eq!(
        stored(recovered).last_seen_at(),
        instant(ISSUED_AT + 5_001),
        "a rejected validation must not have advanced the recorded activity"
    );
}

#[test]
fn rotating_the_csrf_digest_changes_nothing_else_and_needs_a_usable_session() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();

    let rotated = stored(
        database
            .rotate_csrf(&token(TOKEN_BYTE), &csrf(0x63), instant(ISSUED_AT + 10))
            .unwrap(),
    );
    let reloaded = stored(
        database
            .validate_and_touch(&token(TOKEN_BYTE), &csrf(0x63), instant(ISSUED_AT + 11))
            .unwrap(),
    );
    let unknown = database
        .rotate_csrf(&token(0x99), &csrf(0x63), instant(ISSUED_AT + 12))
        .unwrap();
    let expired = database
        .rotate_csrf(
            &token(TOKEN_BYTE),
            &csrf(0x64),
            instant(ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS),
        )
        .unwrap();

    assert!(rotated.csrf_hash().matches(&csrf(0x63)));
    assert!(!rotated.csrf_hash().matches(&csrf(CSRF_BYTE)));
    assert!(reloaded.csrf_hash().matches(&csrf(0x63)));
    assert_eq!(reloaded.issued_at(), instant(ISSUED_AT));
    assert_eq!(
        reloaded.absolute_expires_at(),
        instant(ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS)
    );
    assert_eq!(rejection(unknown), SessionRejection::Unknown);
    assert_eq!(rejection(expired), SessionRejection::AbsoluteLifetime);
}

#[test]
fn revocation_removes_only_the_named_session_or_account() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(0x01, 0x41)).unwrap();
    database.create(&new_session(0x02, 0x41)).unwrap();
    database.create(&new_session(0x03, 0x42)).unwrap();

    let revoked = database.revoke(&token(0x01)).unwrap();
    let revoked_again = database.revoke(&token(0x01)).unwrap();
    let for_account = database.revoke_for_account(account(0x41)).unwrap();
    let for_absent_account = database.revoke_for_account(account(0x43)).unwrap();

    assert!(revoked);
    assert!(!revoked_again);
    assert_eq!(for_account, 1);
    assert_eq!(for_absent_account, 0);
    assert_eq!(session_count(&path), 1);
    assert_eq!(
        stored(
            database
                .validate_and_touch(&token(0x03), &csrf(CSRF_BYTE), instant(ISSUED_AT + 1))
                .unwrap()
        )
        .account(),
        account(0x42)
    );
}

#[test]
fn purging_removes_exactly_the_sessions_past_a_boundary() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(0x01, 0x41)).unwrap();
    database.create(&new_session(0x02, 0x41)).unwrap();
    stored(
        database
            .validate_and_touch(&token(0x02), &csrf(CSRF_BYTE), instant(ISSUED_AT + 1))
            .unwrap(),
    );
    let idle_deadline = ISSUED_AT + SESSION_IDLE_TIMEOUT_MILLISECONDS;

    let before_boundary = database.purge_expired(instant(idle_deadline - 1)).unwrap();
    let at_boundary = database.purge_expired(instant(idle_deadline)).unwrap();
    let after_boundary = database.purge_expired(instant(idle_deadline + 1)).unwrap();

    assert_eq!(before_boundary, 0);
    assert_eq!(at_boundary, 1, "only the session idle since issue expires");
    assert_eq!(after_boundary, 1);
    assert_eq!(session_count(&path), 0);
}

#[test]
fn purging_also_removes_a_session_past_only_its_absolute_lifetime() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    let absolute_deadline = ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS;
    keep_active_until(&mut database, &token(TOKEN_BYTE), absolute_deadline - 1);

    let before_boundary = database
        .purge_expired(instant(absolute_deadline - 1))
        .unwrap();
    let at_boundary = database.purge_expired(instant(absolute_deadline)).unwrap();

    assert_eq!(before_boundary, 0);
    assert_eq!(at_boundary, 1);
}

#[test]
fn the_schema_cannot_hold_a_plaintext_token_or_csrf_value() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    drop(database);
    let connection = Connection::open(&path).unwrap();
    // Forty-three characters is the encoded length of a bearer token.
    let plaintext = "0123456789012345678901234567890123456789012";

    for statement in [
        "INSERT INTO weavelit_session VALUES (?1, ?1, x'000102030405060708090A0B0C0D0E0F', \
         'web-ui', 0, 0, 43200000)",
        "UPDATE weavelit_session SET csrf_hash = ?1",
    ] {
        let error = connection.execute(statement, rusqlite::params![plaintext]);
        assert!(
            error.is_err(),
            "the schema must refuse a plaintext value for {statement}"
        );
    }

    assert!(
        connection
            .execute(
                "INSERT INTO weavelit_session VALUES (zeroblob(32), zeroblob(32), \
                 x'000102030405060708090A0B0C0D0E0F', 'web-ui', 0, 0, 43200000)",
                [],
            )
            .is_err(),
        "the reserved all-zero digest must be refused"
    );
    assert_eq!(session_count(&path), 1);
}

#[test]
fn the_stored_lifetime_cannot_be_extended_or_reassigned_by_direct_statement() {
    let temporary_directory = tempfile::tempdir().unwrap();
    let path = database_path(&temporary_directory);
    let mut database = opened(&path);
    database.create(&new_session(TOKEN_BYTE, 0x41)).unwrap();
    drop(database);
    let connection = Connection::open(&path).unwrap();

    for statement in [
        "UPDATE weavelit_session SET absolute_expires_at_milliseconds = \
         absolute_expires_at_milliseconds + 1",
        "UPDATE weavelit_session SET issued_at_milliseconds = issued_at_milliseconds + 1",
        "UPDATE weavelit_session SET account_id = x'0102030405060708090A0B0C0D0E0F10'",
        "UPDATE weavelit_session SET client_module = 'other'",
        "UPDATE weavelit_session SET last_seen_at_milliseconds = 0",
    ] {
        assert!(
            connection.execute(statement, []).is_err(),
            "the store must refuse {statement}"
        );
    }

    let mut reopened = opened(&path);
    let session = stored(
        reopened
            .validate_and_touch(&token(TOKEN_BYTE), &csrf(CSRF_BYTE), instant(ISSUED_AT + 1))
            .unwrap(),
    );

    assert_eq!(session.issued_at(), instant(ISSUED_AT));
    assert_eq!(
        session.absolute_expires_at(),
        instant(ISSUED_AT + SESSION_ABSOLUTE_LIFETIME_MILLISECONDS)
    );
}
