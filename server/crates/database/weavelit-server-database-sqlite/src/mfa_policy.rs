use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, DatabaseError, MfaPolicyAction, MfaPolicyAuditTerminalWrites,
    MfaPolicyMutation, MfaPolicyMutationOutcome, MfaPolicyRecheck, MfaPolicyTarget,
    MfaPolicyWriterStore, Name, STATE_IDENTIFIER_LENGTH, StateIdentifier,
};

use crate::error::{ErrorContext, map_sqlite_error};
use crate::{SqliteDatabase, audit_recovery, mfa, session};

const SELECT_POLICY_TARGET: &str = "SELECT identity.account_id, account.mfa_required, \
     reference.audit_reference, \
     (SELECT factor_id FROM weavelit_mfa_factor \
       WHERE account_id = identity.account_id AND module = ?2) \
     FROM weavelit_account_public_identity AS identity \
     JOIN weavelit_account AS account ON account.account_id = identity.account_id \
     JOIN weavelit_account_audit_reference AS reference \
       ON reference.account_id = identity.account_id \
     WHERE identity.public_identifier = ?1";
const SELECT_TARGET_STATE: &str = "SELECT account.mfa_required, \
     (SELECT factor_id FROM weavelit_mfa_factor \
       WHERE account_id = account.account_id AND module = ?3) \
     FROM weavelit_account AS account \
     WHERE account.account_id = ?1 \
       AND EXISTS(SELECT 1 FROM weavelit_account_public_identity \
                   WHERE account_id = ?1 AND public_identifier = ?2)";
const SELECT_ACTOR_ACTIVE: &str = "SELECT active FROM weavelit_account WHERE account_id = ?1";
const CHANGE_REQUIREMENT: &str = "UPDATE weavelit_account SET mfa_required = ?3 \
     WHERE account_id = ?1 AND mfa_required = ?2";
const DELETE_WATERMARK: &str = "DELETE FROM weavelit_mfa_replay_watermark WHERE factor_id = ?1";
const DELETE_FACTOR: &str = "DELETE FROM weavelit_mfa_factor \
     WHERE factor_id = ?1 AND account_id = ?2 AND module = ?3";
const DELETE_TARGET_SESSIONS: &str = "DELETE FROM weavelit_session WHERE account_id = ?1";
type PolicyTargetRow = (Vec<u8>, i64, String, Option<Vec<u8>>);

impl MfaPolicyWriterStore for SqliteDatabase {
    fn prepare_mfa_policy_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        module: &Name,
        target: AccountPublicIdentifier,
    ) -> Result<Option<MfaPolicyTarget>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let row: Option<PolicyTargetRow> = transaction
            .query_row(
                SELECT_POLICY_TARGET,
                params![
                    public_identifier_persistence.encode(&target).as_slice(),
                    module.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        let prepared = row
            .map(|(account, required, audit_reference, factor)| {
                let account = identifier(&account)?;
                let audit_reference = audit_reference_persistence
                    .decode(&audit_reference)
                    .map_err(|_| DatabaseError::IntegrityFailure)?;
                MfaPolicyTarget::from_persistence(
                    public_identifier_persistence,
                    audit_reference_persistence,
                    target,
                    account,
                    AccountAuditReference::new(account, audit_reference),
                    boolean(required)?,
                    factor.as_deref().map(identifier).transpose()?,
                )
                .map_err(|_| DatabaseError::IntegrityFailure)
            })
            .transpose()?;
        transaction
            .commit()
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        Ok(prepared)
    }

    fn change_mfa_policy(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        mutation: &MfaPolicyMutation,
        audit_terminals: &MfaPolicyAuditTerminalWrites<'_>,
    ) -> Result<MfaPolicyMutationOutcome, DatabaseError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(MfaPolicyMutationOutcome::Denied);
        }
        if !target_matches(&transaction, public_identifier_persistence, mutation)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(MfaPolicyMutationOutcome::Stale);
        }

        let target = mutation.target();
        let revoked_sessions = match mutation.action() {
            MfaPolicyAction::Requirement { required } => {
                let changed = transaction
                    .execute(
                        CHANGE_REQUIREMENT,
                        params![
                            target.account().as_bytes().as_slice(),
                            i64::from(target.required()),
                            i64::from(required),
                        ],
                    )
                    .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
                if changed != 1 {
                    return Err(DatabaseError::IntegrityFailure);
                }
                if required {
                    revoke_sessions(&transaction, target.account())?
                } else {
                    0
                }
            }
            MfaPolicyAction::EnrollmentReset => {
                let factor = target.factor().ok_or(DatabaseError::IntegrityFailure)?;
                transaction
                    .execute(DELETE_WATERMARK, [factor.as_bytes().as_slice()])
                    .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
                let deleted = transaction
                    .execute(
                        DELETE_FACTOR,
                        params![
                            factor.as_bytes().as_slice(),
                            target.account().as_bytes().as_slice(),
                            mutation.recheck().target().module.as_str(),
                        ],
                    )
                    .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
                if deleted != 1 {
                    return Err(DatabaseError::IntegrityFailure);
                }
                revoke_sessions(&transaction, target.account())?
            }
        };
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(MfaPolicyMutationOutcome::Changed { revoked_sessions })
    }
}

fn accept_issuer(
    transaction: &Transaction<'_>,
    recheck: &MfaPolicyRecheck,
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
        .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
    Ok(active.map(boolean).transpose()? == Some(true)
        && mfa::factor_for_account(transaction, recheck.actor(), recheck.target())?
            == Some(recheck.factor())
        && mfa::enabled(transaction, recheck.target())?)
}

fn target_matches(
    transaction: &Transaction<'_>,
    public_identifier_persistence: &AccountPublicIdentifierPersistence,
    mutation: &MfaPolicyMutation,
) -> Result<bool, DatabaseError> {
    let target = mutation.target();
    let current: Option<(i64, Option<Vec<u8>>)> = transaction
        .query_row(
            SELECT_TARGET_STATE,
            params![
                target.account().as_bytes().as_slice(),
                public_identifier_persistence
                    .encode(&target.public_identifier())
                    .as_slice(),
                mutation.recheck().target().module.as_str(),
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))?;
    let Some((required, factor)) = current else {
        return Ok(false);
    };
    Ok(boolean(required)? == target.required()
        && factor.as_deref().map(identifier).transpose()? == target.factor())
}

fn revoke_sessions(
    transaction: &Transaction<'_>,
    account: StateIdentifier,
) -> Result<usize, DatabaseError> {
    transaction
        .execute(DELETE_TARGET_SESSIONS, [account.as_bytes().as_slice()])
        .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(error, ErrorContext::Mfa))
}

fn identifier(bytes: &[u8]) -> Result<StateIdentifier, DatabaseError> {
    let bytes: [u8; STATE_IDENTIFIER_LENGTH] = bytes
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    StateIdentifier::from_bytes(bytes).map_err(|_| DatabaseError::IntegrityFailure)
}

fn boolean(value: i64) -> Result<bool, DatabaseError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}
