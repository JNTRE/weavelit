use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    CredentialRevision, DatabaseError, PasswordChangeAuditTerminalWrites, PasswordChangeMutation,
    PasswordChangeOutcome, PasswordChangeWriterStore, PasswordVerifier, SessionPosture,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::session;

const SELECT_ACCOUNT_STATE: &str = "SELECT account.active, account.credential_revision, \
     account.must_change_password, account.temporary_credential_expires_at_milliseconds, \
     verifier.encoded_verifier FROM weavelit_account AS account \
     JOIN weavelit_password_verifier AS verifier ON verifier.account_id = account.account_id \
     WHERE account.account_id = ?1";
const COMPLETE_ACCOUNT_CHANGE: &str = "UPDATE weavelit_account SET credential_revision = ?2, \
     must_change_password = 0, temporary_credential_expires_at_milliseconds = NULL \
     WHERE account_id = ?1 AND active = 1 AND credential_revision = ?3 \
       AND must_change_password = 1 AND temporary_credential_expires_at_milliseconds > ?4";
const REPLACE_VERIFIER: &str = "UPDATE weavelit_password_verifier SET encoded_verifier = ?3 \
     WHERE account_id = ?1 AND encoded_verifier = ?2";
const DELETE_ACCOUNT_SESSIONS: &str = "DELETE FROM weavelit_session WHERE account_id = ?1";

type AccountStateRow = (i64, Vec<u8>, i64, Option<i64>, String);

impl PasswordChangeWriterStore for SqliteDatabase {
    fn change_password(
        &mut self,
        mutation: &PasswordChangeMutation,
        audit_terminals: &PasswordChangeAuditTerminalWrites<'_>,
    ) -> Result<PasswordChangeOutcome, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::PasswordChange))?;
        if !accept_change(&transaction, mutation)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(PasswordChangeOutcome::Denied);
        }

        let recheck = mutation.recheck();
        exact_one(
            &transaction,
            COMPLETE_ACCOUNT_CHANGE,
            params![
                recheck.account().as_bytes().as_slice(),
                mutation.next_revision().to_stored_bytes().as_slice(),
                recheck.expected_revision().to_stored_bytes().as_slice(),
                recheck.now().as_unix_milliseconds(),
            ],
        )?;
        exact_one(
            &transaction,
            REPLACE_VERIFIER,
            params![
                recheck.account().as_bytes().as_slice(),
                recheck.expected_verifier().as_str(),
                mutation.replacement().verifier.as_str(),
            ],
        )?;
        let revoked_sessions = transaction
            .execute(
                DELETE_ACCOUNT_SESSIONS,
                [recheck.account().as_bytes().as_slice()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::PasswordChange))?;
        if !session::account_allows_issuance(&transaction, mutation.fresh_session())? {
            return Err(DatabaseError::IntegrityFailure);
        }
        session::insert(&transaction, mutation.fresh_session())?;
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(PasswordChangeOutcome::Changed { revoked_sessions })
    }
}

fn accept_change(
    transaction: &Transaction<'_>,
    mutation: &PasswordChangeMutation,
) -> Result<bool, DatabaseError> {
    let recheck = mutation.recheck();
    let Some(stored_session) = session::load(transaction, recheck.session())? else {
        return Ok(false);
    };
    if stored_session.account() != recheck.account()
        || stored_session.client_module() != recheck.client_module()
        || stored_session.posture() != SessionPosture::PasswordChangeRequired
        || stored_session.rejection_at(recheck.now()).is_some()
    {
        return Ok(false);
    }

    let state: Option<AccountStateRow> = transaction
        .query_row(
            SELECT_ACCOUNT_STATE,
            [recheck.account().as_bytes().as_slice()],
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
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::PasswordChange))?;
    let Some((active, revision, must_change, expiration, verifier)) = state else {
        return Ok(false);
    };
    let verifier = PasswordVerifier::new(verifier).map_err(|_| DatabaseError::IntegrityFailure)?;
    Ok(boolean(active)?
        && credential_revision(&revision)? == recheck.expected_revision()
        && boolean(must_change)?
        && expiration.is_some_and(|value| value > recheck.now().as_unix_milliseconds())
        && verifier == *recheck.expected_verifier())
}

fn exact_one(
    transaction: &Transaction<'_>,
    sql: &str,
    parameters: &[&dyn rusqlite::ToSql],
) -> Result<(), DatabaseError> {
    let changed = transaction
        .execute(sql, parameters)
        .map_err(|error| map_sqlite_error(error, ErrorContext::PasswordChange))?;
    if changed != 1 {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(error, ErrorContext::PasswordChange))
}

fn credential_revision(bytes: &[u8]) -> Result<CredentialRevision, DatabaseError> {
    let bytes = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    CredentialRevision::from_stored_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn boolean(value: i64) -> Result<bool, DatabaseError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}
