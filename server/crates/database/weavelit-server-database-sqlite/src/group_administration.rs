use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension as _, Transaction, TransactionBehavior, params};
use weavelit_server_database::{
    AccountAdministrationProjection, AccountPublicIdentifierPersistence, AuditReferencePersistence,
    DatabaseError, GroupAdministrationAuditTerminalWrites, GroupAdministrationProjection,
    GroupAdministrationStore, GroupAdministrationTarget, GroupAuditReference, GroupCreateMutation,
    GroupCreateOutcome, GroupDeleteMutation, GroupDeleteOutcome, GroupGrant, GroupPublicIdentifier,
    GroupPublicIdentifierPersistence, GroupUpdateMutation, GroupUpdateOutcome,
};

use crate::SqliteDatabase;
use crate::audit_recovery;
use crate::error::{ErrorContext, map_sqlite_error};
use crate::group_mutation::accept_issuer;
use crate::state::{
    AccountAdministrationRow, account_administration_projection, decode_grant,
    validate_account_public_identities,
};

const LIST_GROUPS: &str = "SELECT identity.public_identifier, target.name, target.description \
    FROM weavelit_group AS target \
    JOIN weavelit_group_public_identity AS identity ON identity.group_id = target.group_id \
    ORDER BY target.name";
const LOOKUP_GROUP: &str = "SELECT identity.public_identifier, target.name, target.description \
    FROM weavelit_group_public_identity AS identity \
    JOIN weavelit_group AS target ON target.group_id = identity.group_id \
    WHERE identity.public_identifier = ?1";
const PREPARE_GROUP: &str = "SELECT target.group_id, identity.public_identifier, \
    reference.audit_reference, target.name, target.description \
    FROM weavelit_group_public_identity AS identity \
    JOIN weavelit_group AS target ON target.group_id = identity.group_id \
    JOIN weavelit_group_audit_reference AS reference ON reference.group_id = target.group_id \
    WHERE identity.public_identifier = ?1";
const LOOKUP_GROUP_IDENTIFIER: &str = "SELECT group_id FROM weavelit_group_public_identity \
    WHERE public_identifier = ?1";
const LIST_GROUP_MEMBERS: &str = "SELECT identity.public_identifier, account.username, \
    account.display_name, account.active, account.mfa_required \
    FROM weavelit_group_membership AS membership \
    JOIN weavelit_account AS account ON account.account_id = membership.account_id \
    JOIN weavelit_account_public_identity AS identity ON identity.account_id = account.account_id \
    WHERE membership.group_id = ?1 \
    ORDER BY account.username, identity.public_identifier";
const LIST_GROUP_GRANTS: &str = "SELECT grant_kind, grant_value \
    FROM weavelit_group_grant WHERE group_id = ?1 ORDER BY grant_kind, grant_value";
const COVERAGE: &str = "SELECT \
    EXISTS(SELECT 1 FROM weavelit_group AS target \
        LEFT JOIN weavelit_group_public_identity AS identity \
        ON identity.group_id = target.group_id WHERE identity.group_id IS NULL) \
    OR EXISTS(SELECT 1 FROM weavelit_group_public_identity AS identity \
        LEFT JOIN weavelit_group AS target \
        ON target.group_id = identity.group_id WHERE target.group_id IS NULL)";
const PUBLIC_IDENTIFIERS: &str =
    "SELECT public_identifier FROM weavelit_group_public_identity ORDER BY group_id";

type ProjectionRow = (Vec<u8>, String, Option<String>);
type TargetRow = (Vec<u8>, Vec<u8>, String, String, Option<String>);
type GrantRow = (String, String);

impl GroupAdministrationStore for SqliteDatabase {
    fn list_group_administration_projections(
        &mut self,
        persistence: &GroupPublicIdentifierPersistence,
    ) -> Result<Vec<GroupAdministrationProjection>, DatabaseError> {
        let transaction = self.connection.transaction().map_err(group_error)?;
        validate_identities(&transaction, persistence)?;
        let result = rows(&transaction, LIST_GROUPS)?
            .into_iter()
            .map(|row| projection(persistence, row))
            .collect::<Result<Vec<_>, _>>()?;
        commit(transaction)?;
        Ok(result)
    }

    fn load_group_administration_projection(
        &mut self,
        persistence: &GroupPublicIdentifierPersistence,
        public_identifier: GroupPublicIdentifier,
    ) -> Result<Option<GroupAdministrationProjection>, DatabaseError> {
        let transaction = self.connection.transaction().map_err(group_error)?;
        validate_identities(&transaction, persistence)?;
        let encoded = persistence.encode(&public_identifier);
        let row = transaction
            .query_row(LOOKUP_GROUP, [encoded.as_slice()], projection_columns)
            .optional()
            .map_err(group_error)?;
        let result = row.map(|row| projection(persistence, row)).transpose()?;
        commit(transaction)?;
        Ok(result)
    }

    fn list_group_member_administration_projections(
        &mut self,
        account_persistence: &AccountPublicIdentifierPersistence,
        group_persistence: &GroupPublicIdentifierPersistence,
        group: GroupPublicIdentifier,
    ) -> Result<Option<Vec<AccountAdministrationProjection>>, DatabaseError> {
        let transaction = self.connection.transaction().map_err(group_error)?;
        validate_identities(&transaction, group_persistence)?;
        validate_account_public_identities(&transaction, account_persistence)?;
        let Some(group) = resolve_group(&transaction, group_persistence, group)? else {
            commit(transaction)?;
            return Ok(None);
        };
        let members = transaction
            .prepare(LIST_GROUP_MEMBERS)
            .map_err(group_error)?
            .query_map([group.as_slice()], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .map_err(group_error)?
            .collect::<Result<Vec<AccountAdministrationRow>, _>>()
            .map_err(group_error)?
            .into_iter()
            .map(|row| account_administration_projection(account_persistence, row))
            .collect::<Result<Vec<_>, _>>()?;
        commit(transaction)?;
        Ok(Some(members))
    }

    fn list_group_grant_administration_projections(
        &mut self,
        group_persistence: &GroupPublicIdentifierPersistence,
        group: GroupPublicIdentifier,
    ) -> Result<Option<Vec<GroupGrant>>, DatabaseError> {
        let transaction = self.connection.transaction().map_err(group_error)?;
        validate_identities(&transaction, group_persistence)?;
        let Some(group) = resolve_group(&transaction, group_persistence, group)? else {
            commit(transaction)?;
            return Ok(None);
        };
        let mut grants = transaction
            .prepare(LIST_GROUP_GRANTS)
            .map_err(group_error)?
            .query_map([group.as_slice()], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(group_error)?
            .collect::<Result<Vec<GrantRow>, _>>()
            .map_err(group_error)?
            .into_iter()
            .map(|(kind, value)| decode_grant(&kind, value))
            .collect::<Result<Vec<_>, _>>()?;
        grants.sort();
        commit(transaction)?;
        Ok(Some(grants))
    }

    fn prepare_group_administration_target(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        public_identifier: GroupPublicIdentifier,
    ) -> Result<Option<GroupAdministrationTarget>, DatabaseError> {
        let transaction = self.connection.transaction().map_err(group_error)?;
        validate_identities(&transaction, public_identity_persistence)?;
        let encoded = public_identity_persistence.encode(&public_identifier);
        let row = transaction
            .query_row(PREPARE_GROUP, [encoded.as_slice()], target_columns)
            .optional()
            .map_err(group_error)?;
        let result = row
            .map(
                |(group, public_identifier, audit_reference, name, description)| {
                    let group = state_identifier(&group)?;
                    let public_identifier =
                        decode_public_identifier(public_identity_persistence, &public_identifier)?;
                    let audit_reference = audit_reference_persistence
                        .decode(&audit_reference)
                        .map_err(|_| DatabaseError::IntegrityFailure)?;
                    Ok(GroupAdministrationTarget::from_persistence(
                        public_identity_persistence,
                        audit_reference_persistence,
                        GroupAuditReference::new(group, audit_reference),
                        GroupAdministrationProjection::from_persistence(
                            public_identity_persistence,
                            public_identifier,
                            text(name)?,
                            description.map(text).transpose()?,
                        ),
                    ))
                },
            )
            .transpose()?;
        commit(transaction)?;
        Ok(result)
    }

    fn create_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        mutation: &GroupCreateMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupCreateOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.denied())?;
            commit(transaction)?;
            return Ok(GroupCreateOutcome::Denied);
        }
        let identity = mutation.public_identity();
        let encoded = public_identity_persistence.encode(&identity.public_identifier());
        let group = mutation.group();
        let reference = mutation.audit_reference();
        let conflict: i64 = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM weavelit_group WHERE group_id = ?1 OR name = ?2) \
                 OR EXISTS(SELECT 1 FROM weavelit_group_public_identity WHERE public_identifier = ?3) \
                 OR EXISTS(SELECT 1 FROM weavelit_account_audit_reference WHERE audit_reference = ?4) \
                 OR EXISTS(SELECT 1 FROM weavelit_group_audit_reference WHERE audit_reference = ?4) \
                 OR EXISTS(SELECT 1 FROM weavelit_log_configuration_audit_reference WHERE audit_reference = ?4)",
                params![
                    group.identifier.as_bytes().as_slice(),
                    group.name.as_str(),
                    encoded.as_slice(),
                    reference.audit_reference().to_string(),
                ],
                |row| row.get(0),
            )
            .map_err(group_error)?;
        if boolean(conflict)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.conflict())?;
            commit(transaction)?;
            return Ok(GroupCreateOutcome::Conflict);
        }
        transaction
            .execute(
                "INSERT INTO weavelit_group (group_id, name, description) VALUES (?1, ?2, ?3)",
                params![
                    group.identifier.as_bytes().as_slice(),
                    group.name.as_str(),
                    group.description.as_ref().map(|value| value.as_str()),
                ],
            )
            .map_err(group_error)?;
        transaction
            .execute(
                "INSERT INTO weavelit_group_public_identity (group_id, public_identifier) VALUES (?1, ?2)",
                params![group.identifier.as_bytes().as_slice(), encoded.as_slice()],
            )
            .map_err(group_error)?;
        transaction
            .execute(
                "INSERT INTO weavelit_group_audit_reference (group_id, audit_reference) VALUES (?1, ?2)",
                params![
                    group.identifier.as_bytes().as_slice(),
                    reference.audit_reference().to_string(),
                ],
            )
            .map_err(group_error)?;
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(GroupCreateOutcome::Created)
    }

    fn update_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &GroupUpdateMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupUpdateOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            persist_denied(&transaction, audit_terminals)?;
            commit(transaction)?;
            return Ok(GroupUpdateOutcome::Denied);
        }
        if !target_is_current(
            &transaction,
            public_identity_persistence,
            audit_reference_persistence,
            mutation.target(),
        )? {
            persist_denied(&transaction, audit_terminals)?;
            commit(transaction)?;
            return Ok(GroupUpdateOutcome::Stale);
        }
        let group = mutation.target().group().group();
        let conflict: i64 = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM weavelit_group WHERE name = ?1 AND group_id <> ?2)",
                params![mutation.name().as_str(), group.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(group_error)?;
        if boolean(conflict)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.conflict())?;
            commit(transaction)?;
            return Ok(GroupUpdateOutcome::Conflict);
        }
        let changed = transaction
            .execute(
                "UPDATE weavelit_group SET name = ?1, description = ?2 WHERE group_id = ?3",
                params![
                    mutation.name().as_str(),
                    mutation.description().map(|value| value.as_str()),
                    group.as_bytes().as_slice(),
                ],
            )
            .map_err(group_error)?;
        if changed != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(GroupUpdateOutcome::Changed)
    }

    fn delete_group(
        &mut self,
        public_identity_persistence: &GroupPublicIdentifierPersistence,
        audit_reference_persistence: &AuditReferencePersistence,
        mutation: &GroupDeleteMutation,
        audit_terminals: &GroupAdministrationAuditTerminalWrites<'_>,
    ) -> Result<GroupDeleteOutcome, DatabaseError> {
        let transaction = immediate(self)?;
        if !accept_issuer(&transaction, mutation.recheck())? {
            persist_denied(&transaction, audit_terminals)?;
            commit(transaction)?;
            return Ok(GroupDeleteOutcome::Denied);
        }
        if !target_is_current(
            &transaction,
            public_identity_persistence,
            audit_reference_persistence,
            mutation.target(),
        )? {
            persist_denied(&transaction, audit_terminals)?;
            commit(transaction)?;
            return Ok(GroupDeleteOutcome::Stale);
        }
        let group = mutation.target().group().group();
        let nonempty: i64 = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM weavelit_group_membership WHERE group_id = ?1) \
                 OR EXISTS(SELECT 1 FROM weavelit_group_grant WHERE group_id = ?1)",
                [group.as_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(group_error)?;
        if boolean(nonempty)? {
            audit_recovery::persist_in_transaction(&transaction, audit_terminals.conflict())?;
            commit(transaction)?;
            return Ok(GroupDeleteOutcome::Nonempty);
        }
        let deleted_reference = transaction
            .execute(
                "DELETE FROM weavelit_group_audit_reference WHERE group_id = ?1",
                [group.as_bytes().as_slice()],
            )
            .map_err(group_error)?;
        let deleted_group = transaction
            .execute(
                "DELETE FROM weavelit_group WHERE group_id = ?1",
                [group.as_bytes().as_slice()],
            )
            .map_err(group_error)?;
        if deleted_reference != 1 || deleted_group != 1 {
            return Err(DatabaseError::IntegrityFailure);
        }
        audit_recovery::persist_in_transaction(&transaction, audit_terminals.succeeded())?;
        commit(transaction)?;
        Ok(GroupDeleteOutcome::Deleted)
    }
}

fn validate_identities(
    connection: &Connection,
    persistence: &GroupPublicIdentifierPersistence,
) -> Result<(), DatabaseError> {
    let incomplete: i64 = connection
        .query_row(COVERAGE, [], |row| row.get(0))
        .map_err(group_error)?;
    if boolean(incomplete)? {
        return Err(DatabaseError::IntegrityFailure);
    }
    let values = connection
        .prepare(PUBLIC_IDENTIFIERS)
        .map_err(group_error)?
        .query_map([], |row| row.get::<_, Vec<u8>>(0))
        .map_err(group_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(group_error)?;
    let identifiers = values
        .iter()
        .map(|value| decode_public_identifier(persistence, value))
        .collect::<Result<Vec<_>, _>>()?;
    if identifiers.iter().copied().collect::<BTreeSet<_>>().len() != identifiers.len() {
        return Err(DatabaseError::IntegrityFailure);
    }
    Ok(())
}

fn resolve_group(
    transaction: &Transaction<'_>,
    persistence: &GroupPublicIdentifierPersistence,
    group: GroupPublicIdentifier,
) -> Result<Option<Vec<u8>>, DatabaseError> {
    let encoded = persistence.encode(&group);
    transaction
        .query_row(LOOKUP_GROUP_IDENTIFIER, [encoded.as_slice()], |row| {
            row.get(0)
        })
        .optional()
        .map_err(group_error)
}

fn target_is_current(
    transaction: &Transaction<'_>,
    public_identity_persistence: &GroupPublicIdentifierPersistence,
    audit_reference_persistence: &AuditReferencePersistence,
    target: &GroupAdministrationTarget,
) -> Result<bool, DatabaseError> {
    let encoded = public_identity_persistence.encode(&target.projection().public_identifier());
    let row = transaction
        .query_row(PREPARE_GROUP, [encoded.as_slice()], target_columns)
        .optional()
        .map_err(group_error)?;
    let Some((group, public_identifier, audit_reference, name, description)) = row else {
        return Ok(false);
    };
    Ok(state_identifier(&group)? == target.group().group()
        && decode_public_identifier(public_identity_persistence, &public_identifier)?
            == target.projection().public_identifier()
        && audit_reference_persistence
            .decode(&audit_reference)
            .map_err(|_| DatabaseError::IntegrityFailure)?
            == target.group().audit_reference()
        && text::<256>(name)? == *target.projection().name()
        && description.map(text::<1024>).transpose()?.as_ref() == target.projection().description())
}

fn persist_denied(
    transaction: &Transaction<'_>,
    terminals: &GroupAdministrationAuditTerminalWrites<'_>,
) -> Result<(), DatabaseError> {
    audit_recovery::persist_in_transaction(transaction, terminals.denied())
}

fn rows(connection: &Connection, query: &str) -> Result<Vec<ProjectionRow>, DatabaseError> {
    connection
        .prepare(query)
        .map_err(group_error)?
        .query_map([], projection_columns)
        .map_err(group_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(group_error)
}

fn projection_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectionRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?))
}

fn target_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<TargetRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn projection(
    persistence: &GroupPublicIdentifierPersistence,
    (public_identifier, name, description): ProjectionRow,
) -> Result<GroupAdministrationProjection, DatabaseError> {
    Ok(GroupAdministrationProjection::from_persistence(
        persistence,
        decode_public_identifier(persistence, &public_identifier)?,
        text(name)?,
        description.map(text).transpose()?,
    ))
}

fn decode_public_identifier(
    persistence: &GroupPublicIdentifierPersistence,
    value: &[u8],
) -> Result<GroupPublicIdentifier, DatabaseError> {
    let bytes = value
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    persistence
        .decode(bytes)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn state_identifier(
    value: &[u8],
) -> Result<weavelit_server_database::StateIdentifier, DatabaseError> {
    let bytes = value
        .try_into()
        .map_err(|_| DatabaseError::IntegrityFailure)?;
    weavelit_server_database::StateIdentifier::from_bytes(bytes)
        .map_err(|_| DatabaseError::IntegrityFailure)
}

fn text<const MAX: usize>(
    value: String,
) -> Result<weavelit_server_database::BoundedText<MAX>, DatabaseError> {
    weavelit_server_database::BoundedText::new(value).map_err(|_| DatabaseError::IntegrityFailure)
}

fn boolean(value: i64) -> Result<bool, DatabaseError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(DatabaseError::IntegrityFailure),
    }
}

fn immediate(database: &mut SqliteDatabase) -> Result<Transaction<'_>, DatabaseError> {
    database
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(group_error)
}

fn commit(transaction: Transaction<'_>) -> Result<(), DatabaseError> {
    transaction.commit().map_err(group_error)
}

fn group_error(error: rusqlite::Error) -> DatabaseError {
    map_sqlite_error(error, ErrorContext::GroupMutation)
}
