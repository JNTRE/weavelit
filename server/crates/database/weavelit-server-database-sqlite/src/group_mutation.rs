use rusqlite::{OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AccountAuditReference, AccountPublicIdentifier, AccountPublicIdentifierPersistence,
    AuditReferencePersistence, DatabaseError, GroupAuditReference, GroupGrant,
    GroupGrantMutationTarget, GroupMembershipMutationTarget, GroupMutationAuditTerminalWrites,
    GroupMutationOutcome, GroupMutationRecheck, GroupMutationStore, GroupMutationTarget,
    PreparedGroupMutation, STATE_IDENTIFIER_LENGTH, StateIdentifier,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::session;

const SELECT_MEMBERSHIP_TARGET: &str = "SELECT membership_group.audit_reference, \
    account.account_id, account_reference.audit_reference, \
    EXISTS(SELECT 1 FROM weavelit_group_membership AS membership \
      WHERE membership.group_id = target_group.group_id \
        AND membership.account_id = account.account_id) \
    FROM weavelit_group AS target_group \
    JOIN weavelit_group_audit_reference AS membership_group \
      ON membership_group.group_id = target_group.group_id \
    JOIN weavelit_account_public_identity AS identity ON identity.public_identifier = ?2 \
    JOIN weavelit_account AS account ON account.account_id = identity.account_id \
    JOIN weavelit_account_audit_reference AS account_reference \
      ON account_reference.account_id = account.account_id \
    WHERE target_group.group_id = ?1";
const SELECT_GRANT_TARGET: &str = "SELECT reference.audit_reference, \
    EXISTS(SELECT 1 FROM weavelit_group_grant AS direct_grant \
      WHERE direct_grant.group_id = target_group.group_id \
        AND direct_grant.grant_kind = ?2 AND direct_grant.grant_value = ?3) \
    FROM weavelit_group AS target_group \
    JOIN weavelit_group_audit_reference AS reference \
      ON reference.group_id = target_group.group_id \
    WHERE target_group.group_id = ?1";
const SELECT_ACTOR_ACTIVE: &str = "SELECT active FROM weavelit_account WHERE account_id = ?1";
const INSERT_MEMBERSHIP: &str = "INSERT INTO weavelit_group_membership (group_id, account_id) \
    VALUES (?1, ?2) ON CONFLICT (group_id, account_id) DO NOTHING";
const DELETE_MEMBERSHIP: &str = "DELETE FROM weavelit_group_membership \
    WHERE group_id = ?1 AND account_id = ?2";
const INSERT_GRANT: &str = "INSERT INTO weavelit_group_grant \
    (group_id, grant_kind, grant_value) VALUES (?1, ?2, ?3) \
    ON CONFLICT (group_id, grant_kind, grant_value) DO NOTHING";
const DELETE_GRANT: &str = "DELETE FROM weavelit_group_grant \
    WHERE group_id = ?1 AND grant_kind = ?2 AND grant_value = ?3";
const ACTIVE_ADMINISTRATOR_AFTER_MEMBERSHIP_REMOVAL: &str = "SELECT EXISTS( \
    SELECT 1 FROM weavelit_account AS account \
    JOIN weavelit_group_membership AS membership ON membership.account_id = account.account_id \
    JOIN weavelit_group_grant AS direct_grant ON direct_grant.group_id = membership.group_id \
    WHERE account.active = 1 \
      AND direct_grant.grant_kind = 'server_administration' AND direct_grant.grant_value = '' \
      AND NOT (membership.group_id = ?1 AND membership.account_id = ?2))";
const ACTIVE_ADMINISTRATOR_AFTER_GRANT_REMOVAL: &str = "SELECT EXISTS( \
    SELECT 1 FROM weavelit_account AS account \
    JOIN weavelit_group_membership AS membership ON membership.account_id = account.account_id \
    JOIN weavelit_group_grant AS direct_grant ON direct_grant.group_id = membership.group_id \
    WHERE account.active = 1 \
      AND direct_grant.grant_kind = 'server_administration' AND direct_grant.grant_value = '' \
      AND direct_grant.group_id <> ?1)";

impl GroupMutationStore for SqliteDatabase {
    fn prepare_group_membership_target(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        group: StateIdentifier,
        account: AccountPublicIdentifier,
    ) -> Result<Option<GroupMembershipMutationTarget>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
        let public_identifier = public_identifier_persistence.encode(&account);
        let row: Option<(String, Vec<u8>, String, i64)> = transaction
            .query_row(
                SELECT_MEMBERSHIP_TARGET,
                params![group.as_bytes().as_slice(), public_identifier.as_slice()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
        let target = row
            .map(
                |(group_reference, account_identifier, account_reference, present)| {
                    let account_identifier = identifier(&account_identifier)?;
                    let group_reference = audit_reference_persistence
                        .decode(&group_reference)
                        .map_err(|_| DatabaseError::IntegrityFailure)?;
                    let account_reference = audit_reference_persistence
                        .decode(&account_reference)
                        .map_err(|_| DatabaseError::IntegrityFailure)?;
                    GroupMembershipMutationTarget::from_persistence(
                        public_identifier_persistence,
                        audit_reference_persistence,
                        GroupAuditReference::new(group, group_reference),
                        account,
                        AccountAuditReference::new(account_identifier, account_reference),
                        boolean(present)?,
                    )
                    .map_err(|_| DatabaseError::IntegrityFailure)
                },
            )
            .transpose()?;
        commit(transaction)?;
        Ok(target)
    }

    fn prepare_group_grant_target(
        &mut self,
        audit_reference_persistence: &AuditReferencePersistence,
        group: StateIdentifier,
        grant: GroupGrant,
    ) -> Result<Option<GroupGrantMutationTarget>, DatabaseError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
        let (kind, value) = encode_grant(&grant);
        let row: Option<(String, i64)> = transaction
            .query_row(
                SELECT_GRANT_TARGET,
                params![group.as_bytes().as_slice(), kind, value],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
        let target = row
            .map(|(reference, present)| {
                let reference = audit_reference_persistence
                    .decode(&reference)
                    .map_err(|_| DatabaseError::IntegrityFailure)?;
                Ok(GroupGrantMutationTarget::from_persistence(
                    audit_reference_persistence,
                    GroupAuditReference::new(group, reference),
                    grant,
                    boolean(present)?,
                ))
            })
            .transpose()?;
        commit(transaction)?;
        Ok(target)
    }

    fn commit_group_mutation(
        &mut self,
        public_identifier_persistence: &AccountPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &PreparedGroupMutation,
        audit_terminals: &GroupMutationAuditTerminalWrites<'_>,
    ) -> Result<GroupMutationOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(GroupMutationOutcome::Denied);
        }
        if !target_is_current(
            &transaction,
            public_identifier_persistence,
            audit_reference_persistence,
            mutation,
        )? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(GroupMutationOutcome::Stale);
        }
        if !mutation.desired() && removal_leaves_no_active_administrator(&transaction, mutation)? {
            audit_recovery::persist_in_transaction(
                &transaction,
                audit_terminals.last_administrator_denied(),
            )?;
            commit(transaction)?;
            return Ok(GroupMutationOutcome::LastAdministratorDenied);
        }

        let changed = mutate(&transaction, mutation)?;
        if changed != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(GroupMutationOutcome::Changed)
    }
}

fn target_is_current(
    transaction: &Transaction<'_>,
    public_identifier_persistence: &AccountPublicIdentifierPersistence,
    audit_reference_persistence: &AuditReferencePersistence,
    mutation: &PreparedGroupMutation,
) -> Result<bool, DatabaseError> {
    match mutation.target() {
        GroupMutationTarget::Membership(target) => {
            let public_identifier =
                public_identifier_persistence.encode(&target.account_public_identifier());
            let row: Option<(String, Vec<u8>, String, i64)> = transaction
                .query_row(
                    SELECT_MEMBERSHIP_TARGET,
                    params![
                        target.group().group().as_bytes().as_slice(),
                        public_identifier.as_slice()
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
                .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
            let Some((group_reference, account, account_reference, present)) = row else {
                return Ok(false);
            };
            let group_reference = audit_reference_persistence
                .decode(&group_reference)
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            let account_reference = audit_reference_persistence
                .decode(&account_reference)
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            Ok(identifier(&account)? == target.account().account()
                && group_reference == target.group().audit_reference()
                && account_reference == target.account().audit_reference()
                && boolean(present)? == target.present())
        }
        GroupMutationTarget::Grant(target) => {
            let (kind, value) = encode_grant(target.grant());
            let row: Option<(String, i64)> = transaction
                .query_row(
                    SELECT_GRANT_TARGET,
                    params![target.group().group().as_bytes().as_slice(), kind, value],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
            let Some((reference, present)) = row else {
                return Ok(false);
            };
            let reference = audit_reference_persistence
                .decode(&reference)
                .map_err(|_| DatabaseError::IntegrityFailure)?;
            Ok(reference == target.group().audit_reference()
                && boolean(present)? == target.present())
        }
    }
}

fn removal_leaves_no_active_administrator(
    transaction: &Transaction<'_>,
    mutation: &PreparedGroupMutation,
) -> Result<bool, DatabaseError> {
    let remains = match mutation.target() {
        GroupMutationTarget::Membership(target) => transaction.query_row(
            ACTIVE_ADMINISTRATOR_AFTER_MEMBERSHIP_REMOVAL,
            params![
                target.group().group().as_bytes().as_slice(),
                target.account().account().as_bytes().as_slice()
            ],
            |row| row.get::<_, i64>(0),
        ),
        GroupMutationTarget::Grant(target)
            if matches!(target.grant(), GroupGrant::ServerAdministration) =>
        {
            transaction.query_row(
                ACTIVE_ADMINISTRATOR_AFTER_GRANT_REMOVAL,
                [target.group().group().as_bytes().as_slice()],
                |row| row.get::<_, i64>(0),
            )
        }
        GroupMutationTarget::Grant(_) => return Ok(false),
    }
    .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
    Ok(!boolean(remains)?)
}

fn mutate(
    transaction: &Transaction<'_>,
    mutation: &PreparedGroupMutation,
) -> Result<usize, DatabaseError> {
    match mutation.target() {
        GroupMutationTarget::Membership(target) => {
            let group = target.group().group();
            let account = target.account().account();
            let parameters = params![group.as_bytes().as_slice(), account.as_bytes().as_slice()];
            transaction.execute(
                if mutation.desired() {
                    INSERT_MEMBERSHIP
                } else {
                    DELETE_MEMBERSHIP
                },
                parameters,
            )
        }
        GroupMutationTarget::Grant(target) => {
            let (kind, value) = encode_grant(target.grant());
            transaction.execute(
                if mutation.desired() {
                    INSERT_GRANT
                } else {
                    DELETE_GRANT
                },
                params![target.group().group().as_bytes().as_slice(), kind, value],
            )
        }
    }
    .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))
}

pub(super) fn accept_issuer(
    transaction: &Transaction<'_>,
    recheck: &GroupMutationRecheck,
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
        .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))?;
    Ok(active == Some(1))
}

fn encode_grant(grant: &GroupGrant) -> (&'static str, &str) {
    match grant {
        GroupGrant::ClientModule(name) => ("client_module", name.as_str()),
        GroupGrant::ServiceModule(name) => ("service_module", name.as_str()),
        GroupGrant::Operation(name) => ("operation", name.as_str()),
        GroupGrant::ServerAdministration => ("server_administration", ""),
    }
}

fn immediate(database: &mut SqliteDatabase) -> Result<Transaction<'_>, DatabaseError> {
    database
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction
        .commit()
        .map_err(|error| map_sqlite_error(error, ErrorContext::GroupMutation))
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
