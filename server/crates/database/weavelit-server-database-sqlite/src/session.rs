use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    BoundedText, DatabaseError, MAX_NAME_LENGTH, NewSession, SESSION_DIGEST_LENGTH,
    SESSION_IDLE_TIMEOUT_MILLISECONDS, STATE_IDENTIFIER_LENGTH, SessionCsrfHash, SessionInstant,
    SessionRejection, SessionStore, SessionTokenHash, SessionValidation, StateIdentifier,
    StoredSession,
};

use crate::SqliteDatabase;
use crate::error::{ErrorContext, map_sqlite_error};

const SELECT_SESSION: &str = "SELECT token_hash, csrf_hash, account_id, client_module, \
     issued_at_milliseconds, last_seen_at_milliseconds, absolute_expires_at_milliseconds \
     FROM weavelit_session WHERE token_hash = ?1";
const INSERT_SESSION: &str = "INSERT INTO weavelit_session \
     (token_hash, csrf_hash, account_id, client_module, issued_at_milliseconds, \
      last_seen_at_milliseconds, absolute_expires_at_milliseconds) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)";
const TOUCH_SESSION: &str =
    "UPDATE weavelit_session SET last_seen_at_milliseconds = ?2 WHERE token_hash = ?1";
const ROTATE_SESSION: &str = "UPDATE weavelit_session \
     SET csrf_hash = ?2, last_seen_at_milliseconds = ?3 WHERE token_hash = ?1";
const DELETE_SESSION: &str = "DELETE FROM weavelit_session WHERE token_hash = ?1";
const DELETE_SESSIONS_FOR_ACCOUNT: &str = "DELETE FROM weavelit_session WHERE account_id = ?1";
const DELETE_EXPIRED_SESSIONS: &str = "DELETE FROM weavelit_session \
     WHERE ?1 >= absolute_expires_at_milliseconds \
     OR ?1 >= last_seen_at_milliseconds + ?2";
const DELETE_EVERY_SESSION: &str = "DELETE FROM weavelit_session";

type SessionRow = (Vec<u8>, Vec<u8>, Vec<u8>, String, i64, i64, i64);

/// Removes every live session.
///
/// This runs inside the caller's transaction. Checkpoint replacement calls it
/// so a Restore's session clearing commits or rolls back with the state
/// replacement itself rather than as a separate step.
pub(super) fn clear(connection: &Connection) -> Result<(), DatabaseError> {
    connection
        .execute(DELETE_EVERY_SESSION, [])
        .map(|_| ())
        .map_err(|error| map_sqlite_error(error, ErrorContext::Session))
}

impl SessionStore for SqliteDatabase {
    fn create(&mut self, session: &NewSession) -> Result<(), DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        transaction
            .execute(
                INSERT_SESSION,
                params![
                    session.token_hash().as_bytes().as_slice(),
                    session.csrf_hash().as_bytes().as_slice(),
                    session.account().as_bytes().as_slice(),
                    session.client_module().as_str(),
                    session.issued_at().as_unix_milliseconds(),
                    session.absolute_expires_at().as_unix_milliseconds(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::Session))?;

        commit(transaction)
    }

    fn validate_and_touch(
        &mut self,
        token_hash: &SessionTokenHash,
        now: SessionInstant,
    ) -> Result<SessionValidation, DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        let validation = match resolve(&transaction, token_hash, now)? {
            Resolved::Rejected(rejection) => SessionValidation::Rejected(rejection),
            Resolved::Usable(session) => {
                execute(
                    &transaction,
                    TOUCH_SESSION,
                    params![token_hash.as_bytes().as_slice(), now.as_unix_milliseconds()],
                )?;
                SessionValidation::Valid(advanced(&session, session.csrf_hash(), now))
            }
        };
        commit(transaction)?;

        Ok(validation)
    }

    fn rotate_csrf(
        &mut self,
        token_hash: &SessionTokenHash,
        csrf_hash: &SessionCsrfHash,
        now: SessionInstant,
    ) -> Result<SessionValidation, DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        let validation = match resolve(&transaction, token_hash, now)? {
            Resolved::Rejected(rejection) => SessionValidation::Rejected(rejection),
            Resolved::Usable(session) => {
                execute(
                    &transaction,
                    ROTATE_SESSION,
                    params![
                        token_hash.as_bytes().as_slice(),
                        csrf_hash.as_bytes().as_slice(),
                        now.as_unix_milliseconds()
                    ],
                )?;
                SessionValidation::Valid(advanced(&session, csrf_hash, now))
            }
        };
        commit(transaction)?;

        Ok(validation)
    }

    fn revoke(&mut self, token_hash: &SessionTokenHash) -> Result<bool, DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        let removed = count(
            &transaction,
            DELETE_SESSION,
            params![token_hash.as_bytes().as_slice()],
        )?;
        commit(transaction)?;

        Ok(removed > 0)
    }

    fn revoke_for_account(&mut self, account: StateIdentifier) -> Result<usize, DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        let removed = count(
            &transaction,
            DELETE_SESSIONS_FOR_ACCOUNT,
            params![account.as_bytes().as_slice()],
        )?;
        commit(transaction)?;

        Ok(removed)
    }

    fn purge_expired(&mut self, now: SessionInstant) -> Result<usize, DatabaseError> {
        let transaction = immediate(&mut self.connection)?;
        let removed = count(
            &transaction,
            DELETE_EXPIRED_SESSIONS,
            params![
                now.as_unix_milliseconds(),
                SESSION_IDLE_TIMEOUT_MILLISECONDS
            ],
        )?;
        commit(transaction)?;

        Ok(removed)
    }
}

/// A presented session after its stored row and lifetime have been judged.
enum Resolved {
    Usable(StoredSession),
    Rejected(SessionRejection),
}

/// Loads and judges a presented session, removing it when it has expired.
///
/// A session rejected because the clock moved backwards is left in place: the
/// clock, not the session, is what is wrong, so the session is refused without
/// being destroyed.
fn resolve(
    transaction: &Transaction<'_>,
    token_hash: &SessionTokenHash,
    now: SessionInstant,
) -> Result<Resolved, DatabaseError> {
    let Some(session) = load(transaction, token_hash)? else {
        return Ok(Resolved::Rejected(SessionRejection::Unknown));
    };

    match session.rejection_at(now) {
        None => Ok(Resolved::Usable(session)),
        Some(SessionRejection::ClockRollback) => {
            Ok(Resolved::Rejected(SessionRejection::ClockRollback))
        }
        Some(rejection) => {
            execute(
                transaction,
                DELETE_SESSION,
                params![token_hash.as_bytes().as_slice()],
            )?;
            Ok(Resolved::Rejected(rejection))
        }
    }
}

/// Reads one stored session and confirms its stored digest in constant time.
///
/// The indexed equality match locates a candidate row. The decision to treat
/// that row as this session's is the constant-time comparison below, so no
/// accept path depends on the storage engine's own byte comparison.
fn load(
    transaction: &Transaction<'_>,
    token_hash: &SessionTokenHash,
) -> Result<Option<StoredSession>, DatabaseError> {
    let row = transaction
        .query_row(
            SELECT_SESSION,
            params![token_hash.as_bytes().as_slice()],
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
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Session))?;

    let Some((stored_token, csrf, account, client_module, issued, last_seen, absolute)): Option<
        SessionRow,
    > = row
    else {
        return Ok(None);
    };

    let stored_token: SessionTokenHash = digest(&stored_token, SessionTokenHash::from_bytes)?;
    if !stored_token.matches(token_hash) {
        return Err(DatabaseError::IntegrityFailure);
    }

    Ok(Some(StoredSession::new(
        digest(&csrf, SessionCsrfHash::from_bytes)?,
        identifier(&account)?,
        BoundedText::<MAX_NAME_LENGTH>::new(client_module)
            .map_err(|_| DatabaseError::IntegrityFailure)?,
        instant(issued)?,
        instant(last_seen)?,
        instant(absolute)?,
    )))
}

/// Rebuilds a stored session with the activity and CSRF digest just written.
fn advanced(
    session: &StoredSession,
    csrf_hash: &SessionCsrfHash,
    now: SessionInstant,
) -> StoredSession {
    StoredSession::new(
        *csrf_hash,
        session.account(),
        session.client_module().clone(),
        session.issued_at(),
        now,
        session.absolute_expires_at(),
    )
}

fn immediate(connection: &mut Connection) -> Result<Transaction<'_>, DatabaseError> {
    connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| map_sqlite_error(error, ErrorContext::Session))
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Session))
}

fn execute(
    transaction: &Transaction<'_>,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<(), DatabaseError> {
    count(transaction, sql, parameters).map(|_| ())
}

fn count(
    transaction: &Transaction<'_>,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<usize, DatabaseError> {
    transaction
        .execute(sql, parameters)
        .map_err(|error| map_sqlite_error(error, ErrorContext::Session))
}

fn digest<T>(
    bytes: &[u8],
    build: impl Fn(
        [u8; SESSION_DIGEST_LENGTH],
    ) -> Result<T, weavelit_server_database::ContractInputError>,
) -> Result<T, DatabaseError> {
    let bytes: [u8; SESSION_DIGEST_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    build(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn identifier(bytes: &[u8]) -> Result<StateIdentifier, DatabaseError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn instant(value: i64) -> Result<SessionInstant, DatabaseError> {
    SessionInstant::from_unix_milliseconds(value).map_err(|_| DatabaseError::IntegrityFailure)
}
