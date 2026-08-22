use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AccountStatus, AccountStatusAuditTerminalWrites, AccountStatusMutation,
    AccountStatusMutationOutcome, AccountStatusRecheck, AccountStatusTarget,
    AccountStatusWriterStore, AuditReferencePersistence, CredentialRevision, DatabaseError,
    STATE_IDENTIFIER_LENGTH, StateIdentifier,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::session;

const SELECT_STATUS_TARGET: &str = "SELECT identity.account_id, account.active, \
     account.credential_revision, reference.audit_reference \
     FROM weavelit_account_public_identity AS identity \
     JOIN weavelit_account AS account ON account.account_id = identity.account_id \
     JOIN weavelit_account_audit_reference AS reference \
       ON reference.account_id = identity.account_id \
     WHERE identity.public_identifier = ?1";
const SELECT_ACTOR_ACTIVE: &str = "SELECT active FROM weavelit_account WHERE account_id = ?1";
const DISABLE_ACCOUNT: &str = "UPDATE weavelit_account SET active = 0, credential_revision = ?3 \
    WHERE account_id = ?1 AND active = 1 AND credential_revision = ?2 \
       AND EXISTS(SELECT 1 FROM weavelit_account_public_identity \
               WHERE account_id = ?1 AND public_identifier = ?4)";
const REENABLE_ACCOUNT: &str = "UPDATE weavelit_account SET active = 1 \
    WHERE account_id = ?1 AND active = 0 AND credential_revision = ?2 \
      AND EXISTS(SELECT 1 FROM weavelit_account_public_identity \
               WHERE account_id = ?1 AND public_identifier = ?3)";
const DELETE_TARGET_SESSIONS: &str = "DELETE FROM weavelit_session WHERE account_id = ?1";

impl AccountStatusWriterStore for SqliteDatabase {
    fn prepare_account_status_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        target: AccountPublicIdentifier,
    ) -> Result<Option<AccountStatusTarget>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        let row: Option<(Vec<u8>, i64, Vec<u8>, String)> = transaction
            .query_row(
                SELECT_STATUS_TARGET,
                [public_identifier_persistence.encode(&target).as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        let prepared = row
            .map(|(account, active, revision, audit_reference)| {
                let account = identifier(&account)?;
                let audit_reference = audit_reference_persistence
                    .decode(&audit_reference)
                    .map_err(|_| DatabaseError::IntegrityFailure)?;
                AccountStatusTarget::from_persistence(
                    public_identifier_persistence,
                    audit_reference_persistence,
                    target,
                    account,
                    AccountAuditReference::new(account, audit_reference),
                    boolean(active)?,
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

    fn change_account_status(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &AccountStatusMutation,
        audit_terminals: &AccountStatusAuditTerminalWrites<'_>,
    ) -> Result<AccountStatusMutationOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(AccountStatusMutationOutcome::Denied);
        }

        let target = mutation.target();
        let public_identifier = public_identifier_persistence.encode(&target.public_identifier());
        let changed = match mutation.desired() {
            AccountStatus::Disabled => transaction.execute(
                DISABLE_ACCOUNT,
                params![
                    target.account().as_bytes().as_slice(),
                    target.credential_revision().to_stored_bytes().as_slice(),
                    mutation.resulting_revision().to_stored_bytes().as_slice(),
                    public_identifier.as_slice(),
                ],
            ),
            AccountStatus::Active => transaction.execute(
                REENABLE_ACCOUNT,
                params![
                    target.account().as_bytes().as_slice(),
                    target.credential_revision().to_stored_bytes().as_slice(),
                    public_identifier.as_slice(),
                ],
            ),
        }
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
        if changed == 0 {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(AccountStatusMutationOutcome::Stale);
        }
        if changed != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }

        let revoked_sessions = if mutation.desired() == AccountStatus::Disabled {
            transaction
                .execute(
                    DELETE_TARGET_SESSIONS,
                    [target.account().as_bytes().as_slice()],
                )
                .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?
        } else {
            0
        };
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(AccountStatusMutationOutcome::Changed { revoked_sessions })
    }
}

fn accept_issuer(
    transaction: &Transaction<'_>,
    recheck: &AccountStatusRecheck,
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

    let active = transaction
        .query_row(
            SELECT_ACTOR_ACTIVE,
            [recheck.actor().as_bytes().as_slice()],
            |row| row.get::<_, i64>(0),
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::AccountWriter))?;
    active
        .map(boolean)
        .transpose()
        .map(|active| active == Some(true))
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
