use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AccountCreateMutation, AccountCreateOutcome, AccountCredentialAuditTerminalWrites,
    AccountCredentialIssuanceFactor, AccountCredentialIssuanceRecheck,
    AccountCredentialWriterStore, AccountPasswordResetMutation, AccountPasswordResetOutcome,
    AccountPasswordResetTarget, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, CredentialRevision, DatabaseError, STATE_IDENTIFIER_LENGTH,
    StateIdentifier,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::{mfa, session};

const SELECT_RESET_TARGET: &str = "SELECT identity.account_id, account.credential_revision, \
     reference.audit_reference FROM weavelit_account_public_identity AS identity \
     JOIN weavelit_account AS account ON account.account_id = identity.account_id \
     JOIN weavelit_account_audit_reference AS reference \
       ON reference.account_id = identity.account_id \
     WHERE identity.public_identifier = ?1";
const SELECT_ACTOR_STATE: &str = "SELECT account.active, account.credential_revision, \
     account.must_change_password, account.temporary_credential_expires_at_milliseconds, \
     EXISTS(SELECT 1 FROM weavelit_password_verifier AS verifier \
            WHERE verifier.account_id = account.account_id) \
     FROM weavelit_account AS account WHERE account.account_id = ?1";
const SELECT_STATE_IDENTIFIER_COLLISION: &str = "SELECT EXISTS( \
     SELECT account_id FROM weavelit_account WHERE account_id = ?1 \
     UNION ALL SELECT group_id FROM weavelit_group WHERE group_id = ?1 \
     UNION ALL SELECT factor_id FROM weavelit_mfa_factor WHERE factor_id = ?1 \
     UNION ALL SELECT connection_id FROM weavelit_service_connection WHERE connection_id = ?1 \
     UNION ALL SELECT configuration_id FROM weavelit_log_module_configuration \
       WHERE configuration_id = ?1)";
const SELECT_USERNAME_COLLISION: &str =
    "SELECT EXISTS(SELECT 1 FROM weavelit_account WHERE username = ?1)";
const SELECT_PUBLIC_IDENTIFIER_COLLISION: &str = "SELECT EXISTS( \
     SELECT 1 FROM weavelit_account_public_identity WHERE public_identifier = ?1)";
const SELECT_AUDIT_REFERENCE_COLLISION: &str = "SELECT EXISTS( \
     SELECT audit_reference FROM weavelit_account_audit_reference WHERE audit_reference = ?1 \
     UNION ALL SELECT audit_reference FROM weavelit_group_audit_reference \
       WHERE audit_reference = ?1 \
     UNION ALL SELECT audit_reference FROM weavelit_log_configuration_audit_reference \
       WHERE audit_reference = ?1)";
const INSERT_ACCOUNT: &str = "INSERT INTO weavelit_account \
     (account_id, username, display_name, active, mfa_required, credential_revision, \
      must_change_password, temporary_credential_expires_at_milliseconds) \
     VALUES (?1, ?2, ?3, 1, 0, ?4, 1, ?5)";
const INSERT_PUBLIC_IDENTITY: &str = "INSERT INTO weavelit_account_public_identity \
     (account_id, public_identifier) VALUES (?1, ?2)";
const INSERT_AUDIT_REFERENCE: &str = "INSERT INTO weavelit_account_audit_reference \
     (account_id, audit_reference) VALUES (?1, ?2)";
const INSERT_PASSWORD_VERIFIER: &str = "INSERT INTO weavelit_password_verifier \
     (account_id, encoded_verifier) VALUES (?1, ?2)";
const UPSERT_PASSWORD_VERIFIER: &str = "INSERT INTO weavelit_password_verifier \
     (account_id, encoded_verifier) VALUES (?1, ?2) \
     ON CONFLICT (account_id) DO UPDATE SET encoded_verifier = excluded.encoded_verifier";
const RESET_ACCOUNT: &str = "UPDATE weavelit_account SET credential_revision = ?4, \
     must_change_password = 1, temporary_credential_expires_at_milliseconds = ?5 \
     WHERE account_id = ?1 AND credential_revision = ?2 \
       AND EXISTS(SELECT 1 FROM weavelit_account_public_identity \
                  WHERE account_id = ?1 AND public_identifier = ?3)";
const DELETE_TARGET_SESSIONS: &str = "DELETE FROM weavelit_session WHERE account_id = ?1";

type ActorStateRow = (i64, Vec<u8>, i64, Option<i64>, i64);

impl AccountCredentialWriterStore for SqliteDatabase {
    fn prepare_password_reset_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountPasswordResetTarget>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        let row: Option<(Vec<u8>, Vec<u8>, String)> = transaction
            .query_row(
                SELECT_RESET_TARGET,
                [public_identifier_persistence.encode(&target).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        let prepared = row
            .map(|(account, revision, audit_reference)| {
                let account = identifier(&account)?;
                let audit_reference = audit_reference_persistence
                    .decode(&audit_reference)
                    .map_err(|_| DatabaseError::IntegrityFailure)?;
                AccountPasswordResetTarget::from_persistence(
                    public_identifier_persistence,
                    audit_reference_persistence,
                    target,
                    account,
                    weavelit_server_database::AccountAuditReference::new(account, audit_reference),
                    credential_revision(&revision)?,
                )
                .map_err(|_| DatabaseError::IntegrityFailure)
            })
            .transpose()?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        Ok(prepared)
    }

    fn create_account(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountCreateMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountCreateOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(AccountCreateOutcome::Denied);
        }
        if create_conflicts(&transaction, public_identifier_persistence, mutation)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.conflict())?;
            commit(transaction)?;
            return Ok(AccountCreateOutcome::Conflict);
        }

        let account = mutation.account();
        transaction
            .execute(
                INSERT_ACCOUNT,
                params![
                    account.identifier.as_bytes().as_slice(),
                    account.username.as_str(),
                    account.display_name.as_ref().map(|name| name.as_str()),
                    account.credential_revision.to_stored_bytes().as_slice(),
                    account
                        .temporary_credential_expiration
                        .ok_or(DatabaseError::IntegrityFailure)?
                        .as_unix_milliseconds(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        transaction
            .execute(
                INSERT_PUBLIC_IDENTITY,
                params![
                    account.identifier.as_bytes().as_slice(),
                    public_identifier_persistence
                        .encode(&mutation.public_identity().public_identifier())
                        .as_slice(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        transaction
            .execute(
                INSERT_AUDIT_REFERENCE,
                params![
                    account.identifier.as_bytes().as_slice(),
                    mutation.audit_reference().audit_reference().to_string(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        transaction
            .execute(
                INSERT_PASSWORD_VERIFIER,
                params![
                    account.identifier.as_bytes().as_slice(),
                    mutation.password_verifier().verifier.as_str(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(AccountCreateOutcome::Created)
    }

    fn reset_account_password(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountPasswordResetMutation,
        audit_terminals: &AccountCredentialAuditTerminalWrites<'_>,
    ) -> Result<AccountPasswordResetOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(AccountPasswordResetOutcome::Denied);
        }

        let target = mutation.target();
        let changed = transaction
            .execute(
                RESET_ACCOUNT,
                params![
                    target.account().as_bytes().as_slice(),
                    target.expected_revision().to_stored_bytes().as_slice(),
                    public_identifier_persistence
                        .encode(&target.public_identifier())
                        .as_slice(),
                    mutation.next_revision().to_stored_bytes().as_slice(),
                    mutation.expiration().as_unix_milliseconds(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        if changed == 0 {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.conflict())?;
            commit(transaction)?;
            return Ok(AccountPasswordResetOutcome::Stale);
        }
        if changed != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }

        transaction
            .execute(
                UPSERT_PASSWORD_VERIFIER,
                params![
                    target.account().as_bytes().as_slice(),
                    mutation.password_verifier().verifier.as_str(),
                ],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        let revoked_sessions = transaction
            .execute(
                DELETE_TARGET_SESSIONS,
                [target.account().as_bytes().as_slice()],
            )
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(AccountPasswordResetOutcome::Reset { revoked_sessions })
    }
}

fn accept_issuer(
    transaction: &Transaction<'_>,
    recheck: &AccountCredentialIssuanceRecheck,
) -> Result<bool, DatabaseError> {
    Ok(credential_issuance_identity_matches(transaction, recheck)?
        && credential_issuance_factor_matches(
            transaction,
            recheck,
            WatermarkRequirement::Accepted,
        )?)
}

pub(super) fn credential_issuance_step_up_matches(
    transaction: &Transaction<'_>,
    recheck: &AccountCredentialIssuanceRecheck,
) -> Result<bool, DatabaseError> {
    Ok(credential_issuance_identity_matches(transaction, recheck)?
        && credential_issuance_factor_matches(transaction, recheck, WatermarkRequirement::Any)?)
}

fn credential_issuance_identity_matches(
    transaction: &Transaction<'_>,
    recheck: &AccountCredentialIssuanceRecheck,
) -> Result<bool, DatabaseError> {
    let Some(stored_session) = session::load(transaction, recheck.session())? else {
        return Ok(false);
    };
    if stored_session.account() != recheck.actor()
        || stored_session.client_module() != recheck.client_module()
        || stored_session.rejection_at(recheck.now()).is_some()
    {
        return Ok(false);
    }

    let actor_state: Option<ActorStateRow> = transaction
        .query_row(
            SELECT_ACTOR_STATE,
            [recheck.actor().as_bytes().as_slice()],
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
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
    let Some((active, revision, must_change, expiration, has_verifier)) = actor_state else {
        return Ok(false);
    };
    if !boolean(active)?
        || credential_revision(&revision)? != recheck.expected_actor_revision()
        || boolean(must_change)?
        || expiration.is_some()
        || !boolean(has_verifier)?
    {
        return Ok(false);
    }

    Ok(true)
}

#[derive(Clone, Copy)]
enum WatermarkRequirement {
    Any,
    Accepted,
}

fn credential_issuance_factor_matches(
    transaction: &Transaction<'_>,
    recheck: &AccountCredentialIssuanceRecheck,
    watermark: WatermarkRequirement,
) -> Result<bool, DatabaseError> {
    match recheck.factor() {
        AccountCredentialIssuanceFactor::NoneObserved {
            target,
            module_enabled,
        } => Ok(
            mfa::factor_for_account(transaction, recheck.actor(), target)?.is_none()
                && mfa::enabled(transaction, target)? == *module_enabled,
        ),
        AccountCredentialIssuanceFactor::Totp {
            target,
            factor,
            verified_step,
        } => {
            let current = mfa::factor_for_account(transaction, recheck.actor(), target)?;
            if current != Some(*factor) || !mfa::enabled(transaction, target)? {
                return Ok(false);
            }
            match watermark {
                WatermarkRequirement::Any => Ok(true),
                WatermarkRequirement::Accepted => {
                    mfa::watermark_matches(transaction, *factor, *verified_step)
                }
            }
        }
    }
}

fn create_conflicts(
    transaction: &Transaction<'_>,
    public_identifier_persistence: &AccountPublicIdentifierPersistence,
    mutation: &AccountCreateMutation,
) -> Result<bool, DatabaseError> {
    let account = mutation.account();
    let state_identifier = exists(
        transaction,
        SELECT_STATE_IDENTIFIER_COLLISION,
        account.identifier.as_bytes().as_slice(),
    )?;
    let username = exists(
        transaction,
        SELECT_USERNAME_COLLISION,
        account.username.as_str(),
    )?;
    let public_identifier = exists(
        transaction,
        SELECT_PUBLIC_IDENTIFIER_COLLISION,
        public_identifier_persistence
            .encode(&mutation.public_identity().public_identifier())
            .as_slice(),
    )?;
    let audit_reference = exists(
        transaction,
        SELECT_AUDIT_REFERENCE_COLLISION,
        mutation.audit_reference().audit_reference().to_string(),
    )?;
    Ok(state_identifier || username || public_identifier || audit_reference)
}

fn exists(
    transaction: &Transaction<'_>,
    sql: &str,
    parameter: impl rusqlite::ToSql,
) -> Result<bool, DatabaseError> {
    transaction
        .query_row(sql, [parameter], |row| row.get::<_, i64>(0))
        .map(|value| value != 0)
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))
}

fn immediate(database: &mut SqliteDatabase) -> Result<Transaction<'_>, DatabaseError> {
    database
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))
}

fn identifier(bytes: &[u8]) -> Result<StateIdentifier, DatabaseError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
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
